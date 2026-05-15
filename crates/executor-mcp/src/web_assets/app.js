// osmcp local web ui — vanilla, no framework, no bundler.
// Observation-only. The agent owns mutations.
// All five tabs render from /api/* polled every 5s while visible.

(function () {
  "use strict";

  const POLL_MS = 5000;
  const STALE_WARN_MS = 10_000;
  const STALE_BAD_MS  = 30_000;

  // ─── State ─────────────────────────────────────────────────
  const S = {
    tab: "portfolio",
    sub: null,           // e.g. {strategy: "<id>"} when drilling in
    lastOk: 0,           // timestamp of last successful fetch
    lastErr: null,
    poller: null,
    inflight: false,
    expanded: new Set(), // expand-state keys (survive re-renders)
    historyFilters: { strategy_id: "", status: "", since: "" },
    // cache of last data so partial-failure re-renders don't blank tabs.
    // `detail` is keyed by strategy_id — populated by pollOnce when on the
    // strategy-detail page so renderStrategyDetail can read synchronously.
    cache: { portfolio: null, strategies: null, policy: null, policyDiff: null, triggers: null, runs: null, detail: {} },
    // Anti-flicker: stable JSON hash of the LAST RENDERED tab payload.
    // pollOnce compares the fresh payload against this — identical ⇒ skip
    // the full renderTab() rebuild so the DOM doesn't blink on no-op ticks.
    lastTabHash: "",
  };

  /// Cheap stable hash of an arbitrary JSON value — used to skip no-op
  /// re-renders. NOT cryptographic; collisions are fine, the cost of a false
  /// skip is one missed render-tick (next poll catches up).
  function jsonHash(v) {
    return JSON.stringify(v);
  }

  // ─── Element shortcuts ─────────────────────────────────────
  const $  = (s, r) => (r || document).querySelector(s);
  const $$ = (s, r) => Array.from((r || document).querySelectorAll(s));
  const el = (tag, attrs, children) => {
    const n = document.createElement(tag);
    if (attrs) for (const k in attrs) {
      if (k === "class") n.className = attrs[k];
      else if (k === "text") n.textContent = attrs[k];
      else if (k === "html") n.innerHTML = attrs[k];
      else if (k.startsWith("on") && typeof attrs[k] === "function")
        n.addEventListener(k.slice(2), attrs[k]);
      else if (attrs[k] != null) n.setAttribute(k, attrs[k]);
    }
    if (children) for (const c of [].concat(children)) {
      if (c == null) continue;
      n.appendChild(typeof c === "string" ? document.createTextNode(c) : c);
    }
    return n;
  };

  // ─── Formatters ────────────────────────────────────────────
  const fmt = {
    // best-effort number formatting; fall back to raw string for things
    // we can't parse without precision loss.
    n(v, opts) {
      if (v === null || v === undefined) return "—";
      const num = typeof v === "number" ? v : Number(v);
      if (!isFinite(num)) return String(v);
      const o = opts || {};
      const min = o.min != null ? o.min : 0;
      // 20 = max fractionDigits Number.toLocaleString accepts — effectively
      // "no cap" for any real value while staying within the API contract.
      // KPI / explicit-precision call sites can still pass `max: 2` etc.
      const max = o.max != null ? o.max : 20;
      return num.toLocaleString(undefined, {
        minimumFractionDigits: min,
        maximumFractionDigits: max,
      });
    },
    usd(v) {
      if (v === null || v === undefined || v === "") return "—";
      const num = typeof v === "number" ? v : Number(v);
      if (!isFinite(num)) return String(v);
      // Precision-by-magnitude: small balances need decimals; large totals
      // become unreadable past two. Bands chosen so a $0.25 entry shows
      // full $0.250000, a $12,345 entry shows $12,345.67, and a $250,000
      // entry shows $250,000.7. Adjust here if the bands feel off.
      const abs = Math.abs(num);
      let max;
      if (abs >= 100000)     max = 1;
      else if (abs >= 10000) max = 2;
      else                   max = 6;
      return "$" + num.toLocaleString(undefined, {
        minimumFractionDigits: 0, maximumFractionDigits: max,
      });
    },
    eth(v) {
      if (v === null || v === undefined || v === "") return "—";
      return fmt.n(v, { max: 6 }) + " ETH";
    },
    wei(v) {
      if (v === null || v === undefined || v === "") return "—";
      return String(v) + " wei";
    },
    micro(v) {
      // micro-units (1e-6) — common for USDC raw / second-since
      if (v === null || v === undefined) return "—";
      const num = Number(v);
      if (!isFinite(num)) return String(v);
      return fmt.n(num / 1e6, { max: 6 });
    },
    bps(v) {
      if (v === null || v === undefined) return "—";
      const num = Number(v);
      if (!isFinite(num)) return String(v);
      return fmt.n(num, { max: 2 }) + " bps";
    },
    pct(v) {
      if (v === null || v === undefined) return "—";
      const num = Number(v);
      if (!isFinite(num)) return String(v);
      return fmt.n(num, { max: 2 }) + "%";
    },
    iso(s) { return s || "—"; },
    rel(s) {
      // relative time: "11m ago", "3h ago", "—" if not parseable
      if (!s) return "—";
      const t = Date.parse(s);
      if (!isFinite(t)) return s;
      const diff = (Date.now() - t) / 1000;
      const abs = Math.abs(diff);
      const suf = diff >= 0 ? " ago" : " from now";
      if (abs < 5)    return "just now";
      if (abs < 60)   return Math.round(abs) + "s" + suf;
      if (abs < 3600) return Math.round(abs / 60) + "m" + suf;
      if (abs < 86400) return Math.round(abs / 3600) + "h" + suf;
      return Math.round(abs / 86400) + "d" + suf;
    },
    shortHex(s, head, tail) {
      if (!s) return "—";
      head = head || 6; tail = tail || 4;
      const stripped = s.startsWith("0x") ? s.slice(2) : s;
      if (stripped.length <= head + tail + 2) return s;
      return s.slice(0, head + 2) + "…" + s.slice(-tail);
    },
  };

  // ─── Heuristics ────────────────────────────────────────────
  const HEX_RE_ADDR = /^0x[0-9a-fA-F]{40}$/;
  const HEX_RE_TX   = /^0x[0-9a-fA-F]{64}$/;
  const ISO_RE      = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/;

  function explorerFor(chain, kind, hash) {
    // kind: "tx" | "address"
    const base = (
      chain === 8453     ? "https://basescan.org" :
      chain === 1        ? "https://etherscan.io" :
      chain === 11155111 ? "https://sepolia.etherscan.io" :
      null
    );
    if (!base) return null;
    return base + "/" + kind + "/" + hash;
  }

  // ─── Renderers for primitive cells ─────────────────────────
  function addrCell(addr, chain) {
    const text = fmt.shortHex(addr, 6, 4);
    const link = explorerFor(chain, "address", addr);
    const a = link
      ? el("a", { href: link, target: "_blank", rel: "noopener", class: "addr mono", title: addr, text })
      : el("span", { class: "addr mono", title: addr, text });
    const btn = el("button", {
      class: "copy",
      title: "copy " + addr,
      onclick: (ev) => {
        ev.preventDefault();
        copyToClipboard(addr, btn);
      },
      text: "copy",
    });
    return el("span", null, [a, btn]);
  }

  function txCell(hash, chain) {
    const text = fmt.shortHex(hash, 8, 6);
    const link = explorerFor(chain, "tx", hash);
    if (link) {
      return el("a", { href: link, target: "_blank", rel: "noopener",
        class: "tx mono", title: hash, text });
    }
    return el("span", { class: "tx mono", title: hash, text });
  }

  function copyToClipboard(text, btn) {
    const done = () => {
      if (!btn) return;
      const prev = btn.textContent;
      btn.classList.add("copied");
      btn.textContent = "copied";
      setTimeout(() => {
        btn.classList.remove("copied");
        btn.textContent = prev;
      }, 900);
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(done, () => fallbackCopy(text, done));
    } else {
      fallbackCopy(text, done);
    }
  }
  function fallbackCopy(text, done) {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed"; ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    try { document.execCommand("copy"); done(); } catch (e) {}
    document.body.removeChild(ta);
  }

  // ─── JSON-to-DOM auto-renderer (plan §6) ───────────────────
  // Render any JSON value into DOM, applying the suffix-based unit
  // conventions and array/object recursion rules. The chain id is
  // threaded through so explorer links pick the right network.
  function renderValue(v, key, chain) {
    if (v === null || v === undefined) {
      return el("span", { class: "dim", text: "—" });
    }
    if (typeof v === "string") return renderScalarString(v, key, chain);
    if (typeof v === "number") return renderScalarNumber(v, key);
    if (typeof v === "boolean") {
      return el("span", { class: "mono", text: v ? "true" : "false" });
    }
    if (Array.isArray(v)) return renderArray(v, key, chain);
    if (typeof v === "object") return renderNestedObject(v, key, chain);
    return el("span", { class: "mono", text: String(v) });
  }

  function renderScalarString(s, key, chain) {
    const lk = (key || "").toLowerCase();
    // address-shaped key OR address-shaped value → addr cell
    if (lk.endsWith("_address") && HEX_RE_ADDR.test(s)) return addrCell(s, chain);
    if (lk.endsWith("_tx_hash") && HEX_RE_TX.test(s))   return txCell(s, chain);
    // RFC3339 timestamp with `_ts` / `_at` key suffix → "Nm ago" + tooltip
    if ((lk.endsWith("_ts") || lk.endsWith("_at")) && ISO_RE.test(s)) {
      return el("span", { class: "mono", title: s, text: fmt.rel(s) });
    }
    // Standalone unit suffixes — value is a string but numeric-shaped
    if (lk.endsWith("_usd") || lk.endsWith("_usdc")) return el("span", { class: "mono", text: fmt.usd(s) });
    if (lk.endsWith("_eth"))   return el("span", { class: "mono", text: fmt.eth(s) });
    if (lk.endsWith("_wei"))   return el("span", { class: "mono", text: fmt.wei(s) });
    if (lk.endsWith("_micro")) return el("span", { class: "mono", text: fmt.micro(s) });
    if (lk.endsWith("_bps"))   return el("span", { class: "mono", text: fmt.bps(s) });
    if (lk.endsWith("_pct"))   return el("span", { class: "mono", text: fmt.pct(s) });
    // Recognise an address / tx hash even without the key hint
    if (HEX_RE_ADDR.test(s)) return addrCell(s, chain);
    if (HEX_RE_TX.test(s))   return txCell(s, chain);
    return el("span", { class: "mono", text: s });
  }

  function renderScalarNumber(n, key) {
    const lk = (key || "").toLowerCase();
    if (lk.endsWith("_usd") || lk.endsWith("_usdc")) return el("span", { class: "mono", text: fmt.usd(n) });
    if (lk.endsWith("_eth"))   return el("span", { class: "mono", text: fmt.eth(n) });
    if (lk.endsWith("_wei"))   return el("span", { class: "mono", text: fmt.wei(n) });
    if (lk.endsWith("_micro")) return el("span", { class: "mono", text: fmt.micro(n) });
    if (lk.endsWith("_bps"))   return el("span", { class: "mono", text: fmt.bps(n) });
    if (lk.endsWith("_pct"))   return el("span", { class: "mono", text: fmt.pct(n) });
    return el("span", { class: "mono", text: fmt.n(n) });
  }

  function renderArray(a, key, chain) {
    if (a.length === 0) return el("span", { class: "dim", text: "empty" });
    const allScalar = a.every((x) =>
      x === null || x === undefined ||
      typeof x === "string" || typeof x === "number" || typeof x === "boolean"
    );
    if (allScalar) {
      const ul = el("ul", { class: "bul" });
      a.forEach((x) => ul.appendChild(el("li", null, [renderValue(x, null, chain)])));
      return ul;
    }
    // Array of objects → consistent-key table when shapes agree.
    const allObjs = a.every((x) => x && typeof x === "object" && !Array.isArray(x));
    if (allObjs) {
      const keys = unionKeys(a);
      const tbl = el("table", { class: "t" });
      const thead = el("thead", null,
        [el("tr", null, keys.map((k) => el("th", { class: numericKey(k) ? "num" : "", text: k })))]);
      tbl.appendChild(thead);
      const tbody = el("tbody");
      a.forEach((row) => {
        const tr = el("tr");
        keys.forEach((k) => {
          const td = el("td", { class: numericKey(k) ? "num mono" : "mono" });
          td.appendChild(renderValue(row[k], k, chain));
          tr.appendChild(td);
        });
        tbody.appendChild(tr);
      });
      tbl.appendChild(tbody);
      return tbl;
    }
    // Mixed shapes — render each entry as its own block.
    const wrap = el("div", { class: "nested" });
    a.forEach((x, i) => {
      wrap.appendChild(el("div", { class: "kv" }, [
        el("div", { class: "k", text: "[" + i + "]" }),
        el("div", { class: "v" }, [renderValue(x, key, chain)]),
      ]));
    });
    return wrap;
  }

  function numericKey(k) {
    const lk = (k || "").toLowerCase();
    return /(_usd|_usdc|_eth|_wei|_micro|_bps|_pct|count|amount|raw|decimals|gas|fee)$/.test(lk);
  }

  function unionKeys(rows) {
    const seen = new Set();
    const order = [];
    rows.forEach((r) => {
      if (!r || typeof r !== "object") return;
      Object.keys(r).forEach((k) => {
        if (!seen.has(k)) { seen.add(k); order.push(k); }
      });
    });
    return order;
  }

  function renderNestedObject(obj, key, chain, opts) {
    opts = opts || {};
    const collapsed = !opts.openByDefault;
    const sk = opts.stateKey || ("obj:" + (key || "") + ":" + JSON.stringify(Object.keys(obj)).slice(0, 80));
    const open = S.expanded.has(sk) || !collapsed;
    const wrap = el("div", { class: open ? "" : "collapsed" });
    const head = el("span", {
      class: "disclose",
      onclick: () => {
        if (S.expanded.has(sk)) S.expanded.delete(sk); else S.expanded.add(sk);
        wrap.classList.toggle("collapsed");
      },
    });
    head.textContent = (open ? "▾" : "▸") + " object (" + Object.keys(obj).length + ")";
    const nested = el("div", { class: "nested" });
    nested.appendChild(renderObjectAsKV(obj, chain));
    wrap.appendChild(head);
    wrap.appendChild(nested);
    return wrap;
  }

  function renderObjectAsKV(obj, chain) {
    const kv = el("div", { class: "kv" });
    Object.keys(obj).forEach((k) => {
      kv.appendChild(el("div", { class: "k", text: k }));
      const vCell = el("div", { class: "v" });
      vCell.appendChild(renderValue(obj[k], k, chain));
      kv.appendChild(vCell);
    });
    return kv;
  }

  // ─── Strategy-records table (v1.10) ────────────────────────
  // The generic renderArray would dump every key into its own column
  // (captured_at | id | payload | record_name | run_id | strategy_id),
  // and the long hex strategy_id alone would break the layout. On a
  // strategy-detail page we already know the strategy and the row id is
  // uninteresting — so collapse the row into:
  //   when | record | summary | run | tx
  // with an expandable detail row carrying the full payload as KV.
  const TOKEN_NAMES_8453 = {
    "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913": "USDC",
    "0x4200000000000000000000000000000000000006": "WETH",
    "0xa238dd80c259a72e81d7e4664a9801593f98d1c5": "Aave V3 Pool",
    "0x2626664c2603336e57b271c5c0b26f421741e481": "Uniswap V3 Router",
    "0x4e65fe4dba92790696d040ac24aa414708f5c0ab": "aUSDC",
  };
  function tokenLabel(addr) {
    if (!addr || typeof addr !== "string") return "";
    const k = addr.toLowerCase();
    return TOKEN_NAMES_8453[k] || (addr.slice(0, 6) + "…" + addr.slice(-4));
  }
  function recordSummary(name, p) {
    if (!p || typeof p !== "object") return "—";
    const asset = tokenLabel(p.asset);
    if (name === "supply" && p.amount_micro != null) {
      return fmt.micro(p.amount_micro) + " " + (asset || "USDC") + " → Aave";
    }
    if (name === "withdraw" && p.amount_micro != null) {
      // amount_micro now comes from logs.Transfer[self].value — real out.
      return fmt.micro(p.amount_micro) + " " + (asset || "USDC") + " ← Aave";
    }
    if (name === "swap") {
      const eth = p.eth_in_wei != null ? fmt.eth(p.eth_in_wei) : "?";
      const usdc = p.usdc_out_micro != null ? fmt.micro(p.usdc_out_micro) : "?";
      return eth + " ETH → " + usdc + " USDC";
    }
    // Fallback: first 2 scalar fields, key=value.
    const out = [];
    for (const k of Object.keys(p)) {
      if (out.length >= 2) break;
      const v = p[k];
      if (v == null || typeof v === "object") continue;
      if (k === "tx_hash" || k === "block" || k === "ts") continue;
      out.push(k + "=" + String(v));
    }
    return out.length ? out.join(" · ") : "—";
  }
  function renderRecordsTable(records, chain) {
    if (!records || records.length === 0) {
      return el("div", { class: "empty", text: "no records yet" });
    }
    const tbl = el("table", { class: "t" });
    tbl.appendChild(el("thead", null, [el("tr", null, [
      el("th"),
      el("th", { text: "when" }),
      el("th", { text: "record" }),
      el("th", { text: "summary" }),
      el("th", { text: "run" }),
      el("th", { text: "tx" }),
    ])]));
    const tbody = el("tbody");
    records.forEach((r, i) => {
      const sk = "rec:" + (r.id != null ? r.id : i);
      const open = S.expanded.has(sk);
      const tr = el("tr", { class: "click" });
      tr.appendChild(el("td", { class: "mono dim", text: open ? "▾" : "▸" }));
      tr.appendChild(el("td", { class: "mono", title: r.captured_at || "",
        text: r.captured_at ? fmt.rel(r.captured_at) : "—" }));
      tr.appendChild(el("td", null,
        [el("span", { class: "chip mono", text: r.record_name || "—" })]));
      tr.appendChild(el("td", { text: recordSummary(r.record_name, r.payload) }));
      tr.appendChild(el("td", { class: "mono",
        text: r.run_id ? fmt.shortHex(r.run_id, 6, 4) : "—",
        title: r.run_id || "" }));
      const tx = r.payload && r.payload.tx_hash;
      const txTd = el("td");
      if (tx) txTd.appendChild(txCell(tx, chain));
      else txTd.appendChild(el("span", { class: "dim", text: "—" }));
      tr.appendChild(txTd);
      tr.addEventListener("click", (ev) => {
        if (ev.target && ev.target.tagName === "A") return;
        if (S.expanded.has(sk)) S.expanded.delete(sk); else S.expanded.add(sk);
        renderTab();
      });
      tbody.appendChild(tr);
      if (open) {
        const detail = el("tr");
        const td = el("td", { colspan: 6 });
        const wrap = el("div", { class: "nested" });
        if (r.payload && typeof r.payload === "object") {
          wrap.appendChild(renderObjectAsKV(r.payload, chain));
        } else {
          wrap.appendChild(el("div", { class: "dim", text: "(empty payload)" }));
        }
        td.appendChild(wrap);
        detail.appendChild(td);
        tbody.appendChild(detail);
      }
    });
    tbl.appendChild(tbody);
    return tbl;
  }

  // ─── v1.13 renderObject ────────────────────────────────────
  //
  // Generic, structure-aware renderer for arbitrary JSON bodies.
  //
  // Three layers:
  //   1. Discovery — top-level keys of `body` are panels (no hardcoded list).
  //   2. Shape inference — scalar-list ⇒ chips, homogeneous object-list ⇒
  //      table, single object ⇒ kv pairs, allow/deny split when present,
  //      empty ⇒ "(none)".
  //   3. Value formatters — dispatch by field-name (chain_id, address,
  //      selector, *_wei, *_at, hash). `opts.field_kinds` from the backend
  //      may extend the defaults (`_field_kinds` envelope, Track P2).
  //
  // Single entry point: renderObject(body, opts). Returns a DOM Element.
  //
  // Wired:
  //   - Policy tab (this track, P1).
  //   - P5 (next wave): strategy view-output panel.
  //   - P4 (next wave): diff lens — `opts.path_prefix` reserved for it.

  const CHAIN_LABELS = {
    1: "Ethereum",
    10: "Optimism",
    8453: "Base",
    42161: "Arbitrum",
    137: "Polygon",
    11155111: "Sepolia",
  };

  const DEFAULT_FIELD_KINDS = {
    chain_id:   ["chain_id", "chain"],
    address:    ["address", "to", "from", "token", "contract", "burner",
                 "pool_addr", "spender", "owner"],
    selector:   ["selector", "fn", "function_selector"],
    wei_amount: ["*_wei", "*_cap_wei", "value_wei", "amount_wei"],
    timestamp:  ["*_at", "*_ts", "created_at", "set_at"],
    hash:       ["tx_hash", "block_hash", "hash"],
  };

  // Order columns for table rendering — chain/address first, then alpha.
  const IMPORTANT_KEYS = [
    "chain", "chain_id", "address", "contract", "token",
    "selector", "fn", "from", "to", "rationale",
  ];

  function titleCaseKey(k) {
    return String(k).replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
  }

  // Merge built-in field-name lists with backend overrides. For each kind,
  // the final allowed set is the union (backend may add new names without
  // a frontend change).
  function mergeFieldKinds(override) {
    const merged = {};
    for (const k of Object.keys(DEFAULT_FIELD_KINDS)) {
      merged[k] = DEFAULT_FIELD_KINDS[k].slice();
    }
    if (override && typeof override === "object") {
      for (const kind of Object.keys(override)) {
        const list = override[kind];
        if (!Array.isArray(list)) continue;
        const target = merged[kind] || (merged[kind] = []);
        for (const name of list) {
          if (typeof name === "string" && target.indexOf(name) < 0) target.push(name);
        }
      }
    }
    return merged;
  }

  // Match a field name against a list that may include `*_suffix` wildcards.
  function fieldNameMatches(name, list) {
    if (!name) return false;
    const lk = String(name).toLowerCase();
    for (const pat of list) {
      if (pat.startsWith("*")) {
        if (lk.endsWith(pat.slice(1).toLowerCase())) return true;
      } else if (lk === pat.toLowerCase()) {
        return true;
      }
    }
    return false;
  }

  // Resolve the formatter kind for a (key, value) pair. The key wins; the
  // value shape only confirms (e.g. address-shaped string under unknown key).
  // `ancestors` is an optional array of parent keys (closest-first) used to
  // resolve transparent wrappers like `allow` / `deny` — a scalar list at
  // `chains.allow` is chain ids, not "allows".
  function kindOf(key, value, kinds, ancestors) {
    // Walk through "structural" wrapper keys (allow/deny/etc.) to find the
    // first key that's actually a field name. This is what makes
    // `chains.allow = [8453]` chain-typed.
    const STRUCTURAL = new Set(["allow", "deny", "allow_global"]);
    const effective = (key != null && !STRUCTURAL.has(String(key).toLowerCase()))
      ? key
      : (ancestors || []).find((a) =>
          a != null && !STRUCTURAL.has(String(a).toLowerCase()));
    if (effective != null) {
      for (const kind of Object.keys(kinds)) {
        if (fieldNameMatches(effective, kinds[kind])) return kind;
      }
      // Singular-from-plural fallback: a list field named `chains` → its
      // elements are chain_ids; `contracts` → addresses; `selectors`.
      const lk = String(effective).toLowerCase();
      if (lk === "chains") return "chain_id";
      if (lk === "contracts" || lk === "tokens" || lk === "addresses") return "address";
      if (lk === "selectors") return "selector";
    }
    // Value-shape fallbacks for unkeyed scalars (chip lists).
    if (typeof value === "string") {
      if (HEX_RE_ADDR.test(value)) return "address";
      if (HEX_RE_TX.test(value))   return "hash";
      if (/^0x[0-9a-fA-F]{8}$/.test(value)) return "selector";
    }
    return null;
  }

  // Render a primitive value according to the resolved formatter kind.
  function formatScalar(value, kind) {
    if (value === null || value === undefined) {
      return el("span", { class: "muted", text: "—" });
    }
    if (kind === "chain_id") {
      const num = Number(value);
      const label = CHAIN_LABELS[num];
      return el("span", { class: "mono",
        text: label ? (label + " · " + num) : String(value) });
    }
    if (kind === "address" || kind === "hash") {
      const s = String(value);
      const short = (kind === "address")
        ? fmt.shortHex(s, 6, 4)
        : fmt.shortHex(s, 8, 6);
      return el("button", {
        class: "addr-copy mono",
        title: s + " — click to copy",
        "data-full": s,
        text: short,
      });
    }
    if (kind === "selector") {
      return el("span", { class: "mono", title: String(value), text: String(value) });
    }
    if (kind === "wei_amount") {
      // v1 fallback: human group-separated string. Strategy-specific
      // decimals would require sibling-`decimals` lookup; we don't have
      // it generically, so just make it parseable.
      const s = String(value);
      if (/^[0-9]+$/.test(s)) {
        return el("span", { class: "mono", title: s + " wei",
          text: s.replace(/\B(?=(\d{3})+(?!\d))/g, "_") + " wei" });
      }
      return el("span", { class: "mono", text: s });
    }
    if (kind === "timestamp") {
      const s = String(value);
      return el("span", { class: "mono", title: s, text: fmt.rel(s) });
    }
    if (typeof value === "boolean") {
      return el("span", { class: "mono", text: value ? "true" : "false" });
    }
    if (typeof value === "number") {
      return el("span", { class: "mono", text: fmt.n(value) });
    }
    return el("span", { class: "mono", text: String(value) });
  }

  // Shape inference for an arbitrary value. Returns one of:
  //   "scalar" | "empty" | "scalar-list" | "object-table" |
  //   "object-kv" | "allow-deny" | "mixed"
  function classifyShape(v) {
    if (v === null || v === undefined) return "empty";
    if (typeof v !== "object") return "scalar";
    if (Array.isArray(v)) {
      if (v.length === 0) return "empty";
      const allScalar = v.every((x) =>
        x === null || x === undefined ||
        typeof x === "string" || typeof x === "number" || typeof x === "boolean");
      if (allScalar) return "scalar-list";
      const allObj = v.every((x) => x && typeof x === "object" && !Array.isArray(x));
      if (allObj) return "object-table";
      return "mixed";
    }
    // Plain object.
    const keys = Object.keys(v);
    if (keys.length === 0) return "empty";
    const hasAllow = "allow" in v;
    const hasDeny  = "deny"  in v;
    if ((hasAllow || hasDeny) && keys.every((k) => k === "allow" || k === "deny" || k === "allow_global")) {
      return "allow-deny";
    }
    return "object-kv";
  }

  function renderEmpty() {
    return el("span", { class: "muted", text: "(none)" });
  }

  function renderScalarList(arr, parentKey, kinds, ancestors) {
    const wrap = el("div", { class: "chips" });
    arr.forEach((v) => {
      const kind = kindOf(parentKey, v, kinds, ancestors);
      // Chips wrap formatted scalars — mostly chain ids, addresses,
      // selectors. addr-copy buttons already look chip-shaped via CSS.
      const chip = el("span", { class: "chip" }, [formatScalar(v, kind)]);
      wrap.appendChild(chip);
    });
    return wrap;
  }

  function renderObjectTable(rows, kinds, ancestors) {
    // Union of keys across rows, with IMPORTANT_KEYS first.
    const seen = new Set();
    rows.forEach((r) => Object.keys(r).forEach((k) => seen.add(k)));
    const cols = [];
    IMPORTANT_KEYS.forEach((k) => { if (seen.has(k)) { cols.push(k); seen.delete(k); } });
    Array.from(seen).sort().forEach((k) => cols.push(k));

    const tbl = el("table", { class: "render-table t" });
    tbl.appendChild(el("thead", null, [el("tr", null,
      cols.map((c) => el("th", { text: c })))]));
    const tbody = el("tbody");
    rows.forEach((r) => {
      const tr = el("tr");
      cols.forEach((c) => {
        const td = el("td", { class: "mono" });
        if (r[c] === undefined) {
          td.appendChild(el("span", { class: "muted", text: "—" }));
        } else {
          td.appendChild(renderAny(r[c], c, kinds, ancestors));
        }
        tr.appendChild(td);
      });
      tbody.appendChild(tr);
    });
    tbl.appendChild(tbody);
    return tbl;
  }

  function renderObjectKV(obj, kinds, ancestors) {
    const kv = el("div", { class: "kv" });
    Object.keys(obj).forEach((k) => {
      kv.appendChild(el("div", { class: "k", text: k }));
      const v = el("div", { class: "v" });
      v.appendChild(renderAny(obj[k], k, kinds, ancestors));
      kv.appendChild(v);
    });
    return kv;
  }

  function renderAllowDeny(obj, parentKey, kinds, ancestors) {
    const wrap = el("div");
    ["allow", "deny"].forEach((slot) => {
      if (!(slot in obj)) return;
      const label = el("div", { class: "render-subhead",
        text: slot[0].toUpperCase() + slot.slice(1) });
      wrap.appendChild(label);
      wrap.appendChild(renderAny(obj[slot], slot, kinds,
        [parentKey].concat(ancestors || []).filter((x) => x != null)));
    });
    if ("allow_global" in obj) {
      const ag = el("div", { class: "kv" });
      ag.appendChild(el("div", { class: "k", text: "allow_global" }));
      const v = el("div", { class: "v" });
      v.appendChild(formatScalar(obj.allow_global, null));
      ag.appendChild(v);
      wrap.appendChild(ag);
    }
    return wrap;
  }

  // Universal renderer — used by panels and recursively by table cells / kv
  // values. `parentKey` is the field name in the containing object (or
  // null at the top of a panel) — it drives both formatter dispatch and
  // wildcard wei/at suffix matching. `ancestors` is the path of parent
  // keys (closest-first) used to see through `allow`/`deny` wrappers.
  function renderAny(v, parentKey, kinds, ancestors) {
    ancestors = ancestors || [];
    const shape = classifyShape(v);
    if (shape === "empty")       return renderEmpty();
    if (shape === "scalar") {
      const kind = kindOf(parentKey, v, kinds, ancestors);
      return formatScalar(v, kind);
    }
    if (shape === "scalar-list") return renderScalarList(v, parentKey, kinds, ancestors);
    const nextAncestors = parentKey != null
      ? [parentKey].concat(ancestors)
      : ancestors;
    if (shape === "object-table") return renderObjectTable(v, kinds, nextAncestors);
    if (shape === "allow-deny")  return renderAllowDeny(v, parentKey, kinds, ancestors);
    if (shape === "object-kv")   return renderObjectKV(v, kinds, nextAncestors);
    // Mixed — fallback to list of recursive renders.
    if (Array.isArray(v)) {
      const wrap = el("div");
      v.forEach((x, i) => {
        const row = el("div", { class: "kv" });
        row.appendChild(el("div", { class: "k", text: "[" + i + "]" }));
        const cell = el("div", { class: "v" });
        cell.appendChild(renderAny(x, parentKey, kinds, ancestors));
        row.appendChild(cell);
        wrap.appendChild(row);
      });
      return wrap;
    }
    return el("span", { class: "mono", text: String(v) });
  }

  function renderObjectPanel(title, key, value, kinds) {
    const shape = classifyShape(value);
    const panel = el("div", { class: "render-panel",
      "data-empty": shape === "empty" ? "true" : "false" });
    panel.appendChild(el("div", { class: "render-panel-title", text: title }));
    const body = el("div", { class: "render-panel-body" });
    body.appendChild(renderAny(value, key, kinds, []));
    panel.appendChild(body);
    return panel;
  }

  // Public entry point.
  //
  //   renderObject(body, opts?)
  //     body  — object | array | scalar (any JSON value)
  //     opts  — {
  //       field_kinds: { kind: [name, "*_suffix", ...], ... }  // backend override
  //       path_prefix: string                                  // reserved for P4 diff
  //     }
  //
  // Returns a DOM Element. Caller owns mounting.
  function renderObject(body, opts) {
    opts = opts || {};
    // `_field_kinds` may travel inside the body (P2 envelope) or via opts.
    const inlineKinds = (body && typeof body === "object" && !Array.isArray(body))
      ? body._field_kinds
      : null;
    const kinds = mergeFieldKinds(opts.field_kinds || inlineKinds || null);

    // Scalar / array / null at the top — render as one anonymous panel.
    if (!body || typeof body !== "object" || Array.isArray(body)) {
      const wrap = el("div", { class: "render-grid" });
      wrap.appendChild(renderObjectPanel("value", null, body, kinds));
      return wrap;
    }

    // Discovery: each top-level key (minus reserved underscore keys) is a
    // panel. NO hardcoded list of allowed dimensions.
    const grid = el("div", { class: "render-grid" });
    const keys = Object.keys(body).filter((k) => !k.startsWith("_"));
    if (keys.length === 0) {
      grid.appendChild(el("div", { class: "muted", text: "(empty)" }));
      return grid;
    }
    keys.forEach((k) => {
      // Pass the raw key (`chains`) — not the title-cased label — into the
      // renderer so kindOf can recognise it (and singular-from-plural
      // inference: chains → chain_id elements).
      grid.appendChild(renderObjectPanel(titleCaseKey(k), k, body[k], kinds));
    });
    return grid;
  }

  // Expose for tests / other modules / P5 reuse.
  window.osmcpRenderObject = renderObject;

  // ─── v1.13 P4 — policy diff lens ──────────────────────────
  //
  // A structure-aware JSON diff that reuses P1's value-formatter
  // dispatch (`formatScalar` + `kindOf`) so chain_id / address /
  // selector / *_wei / *_at / hash render identically in both modes.
  //
  // Public surface:
  //   - diffJson(prev, curr)               → diff tree (see node kinds below)
  //   - renderObjectDiff(diffNode, opts?)  → DOM Element
  //   - window.osmcpDiffJson / window.osmcpRenderObjectDiff (for P5 / tests)
  //
  // Array strategy: by-index. Trusts the backend to emit stable order
  // (policy bodies are deterministic). Shifted order will produce noisy
  // but correct diffs — a future LCS-lite pass is parked for v1.14+.

  // Cheap structural equality. Order-sensitive for arrays. Sorts object
  // keys before comparing so {a,b} === {b,a}.
  function diffDeepEqual(a, b) {
    if (a === b) return true;
    if (a === null || b === null) return false;
    if (typeof a !== typeof b) return false;
    if (typeof a !== "object") return a === b;
    if (Array.isArray(a) !== Array.isArray(b)) return false;
    if (Array.isArray(a)) {
      if (a.length !== b.length) return false;
      for (let i = 0; i < a.length; i++) {
        if (!diffDeepEqual(a[i], b[i])) return false;
      }
      return true;
    }
    const ak = Object.keys(a).sort();
    const bk = Object.keys(b).sort();
    if (ak.length !== bk.length) return false;
    for (let i = 0; i < ak.length; i++) {
      if (ak[i] !== bk[i]) return false;
      if (!diffDeepEqual(a[ak[i]], b[bk[i]])) return false;
    }
    return true;
  }

  // Recursive structural diff. Returns a tree of nodes:
  //   { kind: "unchanged", value }
  //   { kind: "added",     value }
  //   { kind: "removed",   value }
  //   { kind: "changed",   from, to }
  //   { kind: "object",    keys: { <key>: <node>, ... } }
  //   { kind: "array",     entries: [ <node>, ... ] }
  function diffJson(prev, curr) {
    if (diffDeepEqual(prev, curr)) {
      return { kind: "unchanged", value: curr };
    }
    const prevIsObj = prev && typeof prev === "object" && !Array.isArray(prev);
    const currIsObj = curr && typeof curr === "object" && !Array.isArray(curr);
    const prevIsArr = Array.isArray(prev);
    const currIsArr = Array.isArray(curr);

    if (prevIsObj && currIsObj) {
      const keys = new Set();
      Object.keys(prev).forEach((k) => keys.add(k));
      Object.keys(curr).forEach((k) => keys.add(k));
      const out = {};
      keys.forEach((k) => {
        const inPrev = Object.prototype.hasOwnProperty.call(prev, k);
        const inCurr = Object.prototype.hasOwnProperty.call(curr, k);
        if (inPrev && !inCurr) {
          out[k] = { kind: "removed", value: prev[k] };
        } else if (!inPrev && inCurr) {
          out[k] = { kind: "added", value: curr[k] };
        } else {
          out[k] = diffJson(prev[k], curr[k]);
        }
      });
      return { kind: "object", keys: out };
    }

    if (prevIsArr && currIsArr) {
      // By-index. Trusts backend order; shifted lists produce noisy diffs.
      const n = Math.max(prev.length, curr.length);
      const entries = [];
      for (let i = 0; i < n; i++) {
        const hasP = i < prev.length;
        const hasC = i < curr.length;
        if (hasP && !hasC) {
          entries.push({ kind: "removed", value: prev[i] });
        } else if (!hasP && hasC) {
          entries.push({ kind: "added", value: curr[i] });
        } else {
          entries.push(diffJson(prev[i], curr[i]));
        }
      }
      return { kind: "array", entries };
    }

    // Type mismatch or scalar inequality.
    return { kind: "changed", from: prev, to: curr };
  }

  // Count +N ~M -K within a node (for panel badges).
  function diffCounts(node) {
    const acc = { added: 0, changed: 0, removed: 0 };
    function walk(n) {
      if (!n) return;
      switch (n.kind) {
        case "added":     acc.added++;   return;
        case "removed":   acc.removed++; return;
        case "changed":   acc.changed++; return;
        case "object":
          Object.keys(n.keys).forEach((k) => walk(n.keys[k]));
          return;
        case "array":
          n.entries.forEach(walk);
          return;
        default: return;
      }
    }
    walk(node);
    return acc;
  }

  // Format a scalar via P1's dispatch. Thin shim — keeps renderObjectDiff
  // free of formatter knowledge.
  function formatValue(value, parentKey, kinds, ancestors) {
    const kind = kindOf(parentKey, value, kinds, ancestors || []);
    return formatScalar(value, kind);
  }

  // Render a single leaf node (added / removed / changed / unchanged
  // scalar). Uses renderAny for nested values so object/array shapes
  // render with the same kv/table machinery as the non-diff view.
  function renderDiffLeaf(node, parentKey, kinds, ancestors) {
    const ancs = ancestors || [];
    if (node.kind === "unchanged") {
      // Unchanged: render normally — no special styling.
      return renderAny(node.value, parentKey, kinds, ancs);
    }
    if (node.kind === "added") {
      const wrap = el("div", { class: "diff-row diff-added" });
      wrap.appendChild(renderAny(node.value, parentKey, kinds, ancs));
      return wrap;
    }
    if (node.kind === "removed") {
      const wrap = el("div", { class: "diff-row diff-removed" });
      wrap.appendChild(renderAny(node.value, parentKey, kinds, ancs));
      return wrap;
    }
    if (node.kind === "changed") {
      const wrap = el("div", { class: "diff-row diff-changed" });
      const grid = el("div", { class: "diff-changed-grid" });
      const fromCell = el("div", { class: "diff-from" });
      fromCell.appendChild(renderAny(node.from, parentKey, kinds, ancs));
      const arrow = el("div", { class: "diff-arrow", text: "→" });
      const toCell = el("div", { class: "diff-to" });
      toCell.appendChild(renderAny(node.to, parentKey, kinds, ancs));
      grid.appendChild(fromCell);
      grid.appendChild(arrow);
      grid.appendChild(toCell);
      wrap.appendChild(grid);
      return wrap;
    }
    // Container kinds — recurse.
    return renderDiffNode(node, parentKey, kinds, ancs);
  }

  // Render a container node (object / array) into a kv grid or list.
  function renderDiffNode(node, parentKey, kinds, ancestors) {
    const ancs = ancestors || [];
    if (node.kind === "object") {
      const kv = el("div", { class: "kv" });
      const keys = Object.keys(node.keys);
      // Stable key order: alpha — diffs don't need IMPORTANT_KEYS priority.
      keys.sort();
      const nextAncestors = parentKey != null
        ? [parentKey].concat(ancs)
        : ancs;
      keys.forEach((k) => {
        const child = node.keys[k];
        const kCell = el("div", { class: "k", text: k });
        const vCell = el("div", { class: "v" });
        vCell.appendChild(renderDiffLeaf(child, k, kinds, nextAncestors));
        // For added/removed top-level keys, color the key cell too.
        if (child.kind === "added")   kCell.classList.add("diff-added");
        if (child.kind === "removed") kCell.classList.add("diff-removed");
        kv.appendChild(kCell);
        kv.appendChild(vCell);
      });
      return kv;
    }
    if (node.kind === "array") {
      const nextAncestors = parentKey != null
        ? [parentKey].concat(ancs)
        : ancs;
      const wrap = el("div");
      node.entries.forEach((entry, i) => {
        const row = el("div", { class: "kv" });
        row.appendChild(el("div", { class: "k", text: "[" + i + "]" }));
        const cell = el("div", { class: "v" });
        cell.appendChild(renderDiffLeaf(entry, parentKey, kinds, nextAncestors));
        row.appendChild(cell);
        wrap.appendChild(row);
      });
      return wrap;
    }
    // Leaf — defer.
    return renderDiffLeaf(node, parentKey, kinds, ancs);
  }

  function renderDiffCountBadge(counts) {
    const wrap = el("span", { class: "diff-count" });
    wrap.appendChild(el("span", {
      class: "diff-count-add" + (counts.added === 0 ? " diff-count-zero" : ""),
      text: "+" + counts.added,
    }));
    wrap.appendChild(el("span", {
      class: "diff-count-changed" + (counts.changed === 0 ? " diff-count-zero" : ""),
      text: "~" + counts.changed,
    }));
    wrap.appendChild(el("span", {
      class: "diff-count-removed" + (counts.removed === 0 ? " diff-count-zero" : ""),
      text: "-" + counts.removed,
    }));
    return wrap;
  }

  // Public entry point. Renders a top-level diff (object expected as the
  // common case for policy bodies). Mirrors renderObject's layout:
  // one panel per top-level key. Unchanged panels collapse to "(unchanged)".
  function renderObjectDiff(diffNode, opts) {
    opts = opts || {};
    const kinds = mergeFieldKinds(opts.field_kinds || null);

    // Non-object top-level diffs fall back to a single anonymous panel.
    if (!diffNode || diffNode.kind !== "object") {
      const wrap = el("div", { class: "render-grid" });
      const panel = el("div", { class: "render-panel" });
      panel.appendChild(el("div", { class: "render-panel-title", text: "value" }));
      const pb = el("div", { class: "render-panel-body" });
      pb.appendChild(renderDiffLeaf(diffNode, null, kinds, []));
      panel.appendChild(pb);
      wrap.appendChild(panel);
      return wrap;
    }

    const grid = el("div", { class: "render-grid" });
    const keys = Object.keys(diffNode.keys).filter((k) => !k.startsWith("_"));
    if (keys.length === 0) {
      grid.appendChild(el("div", { class: "muted", text: "(empty)" }));
      return grid;
    }
    keys.forEach((k) => {
      const child = diffNode.keys[k];
      const counts = diffCounts(child);
      const isUnchanged = child.kind === "unchanged" ||
        (counts.added === 0 && counts.changed === 0 && counts.removed === 0);
      const panel = el("div", { class: "render-panel" });
      if (isUnchanged) panel.setAttribute("data-diff-unchanged", "true");
      const title = el("div", { class: "render-panel-title", text: titleCaseKey(k) });
      if (!isUnchanged) title.appendChild(renderDiffCountBadge(counts));
      panel.appendChild(title);
      const pb = el("div", { class: "render-panel-body" });
      if (isUnchanged) {
        pb.appendChild(el("span", { text: "(unchanged)" }));
      } else if (child.kind === "added") {
        pb.appendChild(renderDiffLeaf(child, k, kinds, []));
      } else if (child.kind === "removed") {
        pb.appendChild(renderDiffLeaf(child, k, kinds, []));
      } else {
        pb.appendChild(renderDiffNode(child, k, kinds, []));
      }
      panel.appendChild(pb);
      grid.appendChild(panel);
    });
    return grid;
  }

  // Expose for tests / P5 reuse.
  window.osmcpDiffJson         = diffJson;
  window.osmcpRenderObjectDiff = renderObjectDiff;
  // Surface formatValue for any external module that wants to share the
  // same dispatch table (P5 may use it for view-output cells).
  window.osmcpFormatValue      = formatValue;

  // Central click handler for addr-copy buttons. Single delegation —
  // independent of which renderer mounted the button.
  document.addEventListener("click", (e) => {
    const btn = e.target.closest && e.target.closest(".addr-copy");
    if (!btn) return;
    const full = btn.getAttribute("data-full");
    if (!full) return;
    e.preventDefault();
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(full).catch(() => {});
    } else {
      fallbackCopy(full, function () {});
    }
    btn.setAttribute("data-copied", "1");
    setTimeout(() => btn.removeAttribute("data-copied"), 1200);
  });

  // ─── Fetch helper ──────────────────────────────────────────
  async function getJson(path) {
    const res = await fetch(path, { headers: { "Accept": "application/json" } });
    if (!res.ok) {
      let body = null;
      try { body = await res.json(); } catch (e) {}
      const err = new Error("HTTP " + res.status + " for " + path);
      err.status = res.status;
      err.body = body;
      throw err;
    }
    return res.json();
  }

  // ─── Header strip ──────────────────────────────────────────
  function updateHeader(portfolio) {
    if (portfolio) {
      const burner = portfolio.burner;
      if (burner) {
        const span = $("#burner");
        span.innerHTML = "";
        span.appendChild(addrCell(burner, portfolio.chain_id));
      }
      $("#chain").textContent = portfolio.chain_id != null ? String(portfolio.chain_id) : "—";
    }
    refreshFreshness();
  }

  function refreshFreshness() {
    const fresh = $("#freshness");
    const dot = $("#status-dot");
    if (!S.lastOk) {
      fresh.textContent = S.lastErr ? "error" : "loading";
      fresh.className = "freshness" + (S.lastErr ? " stale" : "");
      dot.className = "dot " + (S.lastErr ? "dot-bad" : "dot-idle");
      return;
    }
    const ageMs = Date.now() - S.lastOk;
    const secs = Math.round(ageMs / 1000);
    fresh.textContent = "refreshed " + secs + "s ago";
    let cls = "freshness", dotCls = "dot dot-ok";
    if (ageMs > STALE_BAD_MS)  { cls += " stale"; dotCls = "dot dot-bad"; }
    else if (ageMs > STALE_WARN_MS) { cls += " warn";  dotCls = "dot dot-warn"; }
    fresh.className = cls;
    dot.className = dotCls;
  }
  setInterval(refreshFreshness, 1000);

  // ─── Verdict badge ─────────────────────────────────────────
  function verdictBadge(v) {
    if (!v) return el("span", { class: "badge", text: "—" });
    const cls = (
      v === "satisfied"         ? "ok" :
      v === "aligned"           ? "ok" :
      v === "partial"           ? "warn" :
      v === "partially_aligned" ? "warn" :
      v === "missing"           ? "bad" :
      v === "misaligned"        ? "bad" :
      "partial"
    );
    return el("span", { class: "badge " + cls, text: v });
  }

  // ─── Copy-report helpers ──────────────────────────────────
  // Compose a plain-text report block for an error/anomaly. The user
  // copies it from the UI and pastes it back to the agent (Claude) so
  // diagnosis doesn't require manual screenshotting or hex-id retyping.
  //
  // `kind` is the report family ("policy_alignment", "view_failure", ...).
  // `ctx` is a {label: value} map; values are stringified line-by-line in
  // declaration order. Sub-arrays render as "  - " bullet lists.
  function composeReport(kind, ctx) {
    const at = new Date().toISOString();
    const lines = ["osmcp " + kind + " report (" + at + ")"];
    for (const k of Object.keys(ctx)) {
      const v = ctx[k];
      if (v == null) continue;
      if (Array.isArray(v)) {
        lines.push(k + ":");
        v.forEach((entry) => {
          if (entry == null) return;
          if (typeof entry === "string") {
            lines.push("  - " + entry);
          } else {
            lines.push("  - " + JSON.stringify(entry));
          }
        });
      } else if (typeof v === "object") {
        lines.push(k + ": " + JSON.stringify(v));
      } else {
        lines.push(k + ": " + String(v));
      }
    }
    return lines.join("\n");
  }

  // Compact button — copies the composed report block to the clipboard
  // and flashes "copied" briefly. `stopPropagation` so the button can
  // sit inside a clickable row without triggering navigation.
  function reportBtn(kind, ctx, label) {
    const btn = el("button", {
      class: "copy",
      title: "copy " + kind + " report",
      onclick: (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        copyToClipboard(composeReport(kind, ctx), btn);
      },
      text: label || "copy report",
    });
    return btn;
  }

  // ─── Tab: Portfolio ────────────────────────────────────────
  function renderPortfolio(data) {
    const root = el("div");
    const chain = data && data.chain_id;

    // Header KPIs
    const total = aggregateAssetsTotal(data);
    const kpis = el("div", { class: "kpis" }, [
      kpiCard("Total (USD)", total.totalUsd == null ? "—" : fmt.usd(total.totalUsd)),
      kpiCard("Strategies", String((data.strategies || []).length)),
      kpiCard("Idle balances", String((data.idle_balances || []).length)),
      kpiCard("Refreshed", fmt.rel(data.refreshed_at)),
    ]);
    root.appendChild(kpis);

    // Aggregate $assets table
    const assets = collectAssetRows(data);
    root.appendChild(section("Assets (aggregated)", buildAssetsBody(assets, chain)));

    // Per-strategy cards — non-$assets observations auto-rendered
    const strats = data.strategies || [];
    if (strats.length === 0) {
      root.appendChild(section("Strategies",
        el("div", { class: "empty", html:
          "no strategies registered yet — see <code>docs://strategy-bundle</code> to author one" })));
    } else {
      strats.forEach((s) => {
        root.appendChild(strategyCard(s, chain));
      });
    }
    return root;
  }

  function kpiCard(label, value) {
    return el("div", { class: "kpi" }, [
      el("div", { class: "label", text: label }),
      el("div", { class: "value", text: value }),
    ]);
  }

  function collectAssetRows(portfolio) {
    const rows = [];
    (portfolio.idle_balances || []).forEach((b) => {
      rows.push(Object.assign({ _source: "idle" }, b));
    });
    (portfolio.strategies || []).forEach((s) => {
      const view = s.view_output || {};
      const data = view.data || view;
      const list = (data && data.$assets) || [];
      list.forEach((a) => rows.push(Object.assign({ _source: s.name || s.id }, a)));
    });
    return rows;
  }

  function aggregateAssetsTotal(portfolio) {
    let totalUsd = 0;
    let any = false;
    const rows = collectAssetRows(portfolio);
    rows.forEach((r) => {
      if (typeof r.usd === "number" && isFinite(r.usd)) { totalUsd += r.usd; any = true; }
    });
    return { totalUsd: any ? totalUsd : null, count: rows.length };
  }

  function buildAssetsBody(rows, chain) {
    if (rows.length === 0) {
      return el("div", { class: "empty", text: "no positions reported" });
    }
    // Dedup detection — same (chain_id, venue, asset, address) across rows.
    const seen = {};
    rows.forEach((r) => {
      const k = [r.chain_id, r.venue, r.asset, r.address || ""].join("|");
      seen[k] = (seen[k] || 0) + 1;
    });

    const tbl = el("table", { class: "t" });
    const thead = el("thead", null, [el("tr", null, [
      el("th", { text: "source" }),
      el("th", { text: "chain" }),
      el("th", { text: "venue" }),
      el("th", { text: "asset" }),
      el("th", { text: "address" }),
      el("th", { class: "num", text: "amount" }),
      el("th", { class: "num", text: "usd" }),
    ])]);
    const tbody = el("tbody");
    rows.forEach((r) => {
      const k = [r.chain_id, r.venue, r.asset, r.address || ""].join("|");
      const dup = seen[k] > 1;
      const tr = el("tr");
      tr.appendChild(el("td", { text: r._source || "—" }));
      tr.appendChild(el("td", { class: "mono", text: r.chain_id != null ? String(r.chain_id) : "—" }));
      tr.appendChild(el("td", { text: r.venue || "—" }));
      const assetCell = el("td");
      assetCell.appendChild(document.createTextNode(r.asset || "—"));
      if (dup) {
        assetCell.appendChild(document.createTextNode(" "));
        assetCell.appendChild(el("span", { class: "badge warn", title: "same (chain, venue, asset, address) declared by multiple sources", text: "dup" }));
      }
      tr.appendChild(assetCell);
      const addrTd = el("td", { class: "mono" });
      addrTd.appendChild(r.address ? addrCell(r.address, r.chain_id || chain) : document.createTextNode("—"));
      tr.appendChild(addrTd);
      tr.appendChild(el("td", { class: "num mono", text: r.amount != null ? String(r.amount) : "—" }));
      tr.appendChild(el("td", { class: "num mono",
        text: r.usd != null ? fmt.usd(r.usd) : "—" }));
      tbody.appendChild(tr);
    });
    tbl.appendChild(thead);
    tbl.appendChild(tbody);
    const wrap = el("div", { class: "section-body flush" });
    wrap.appendChild(tbl);
    return wrap;
  }

  function strategyCard(s, chain) {
    const view = s.view_output || {};
    const data = view.data || view;
    const conf = view.confidence;
    const obs = {};
    if (data && typeof data === "object" && !Array.isArray(data)) {
      Object.keys(data).forEach((k) => { if (k !== "$assets") obs[k] = data[k]; });
    }
    const title = el("div", null, [
      el("span", { text: s.name || s.id }),
      " ",
      conf && conf !== "full" ? el("span", { class: "badge partial", text: conf }) : null,
    ]);
    const head = el("div", { class: "section-head" }, [
      title,
      el("span", { class: "mono dim", text: fmt.shortHex(s.id, 6, 4), title: s.id }),
    ]);
    const body = el("div", { class: "section-body" });
    if (view.reason) {
      body.appendChild(el("div", { class: "dim", text: view.reason }));
    }
    if (Object.keys(obs).length === 0) {
      body.appendChild(el("div", { class: "empty", text: "no observations" }));
    } else {
      body.appendChild(renderObjectAsKV(obs, chain));
    }
    const card = el("div", { class: "section" });
    card.appendChild(head);
    card.appendChild(body);
    return card;
  }

  function section(title, child, opts) {
    opts = opts || {};
    const head = el("div", { class: "section-head" }, [
      el("span", { text: title }),
      opts.aside || null,
    ]);
    const body = (child && child.classList && child.classList.contains("section-body"))
      ? child
      : (function () {
          const b = el("div", { class: "section-body" });
          b.appendChild(child);
          return b;
        })();
    const sec = el("div", { class: "section" });
    sec.appendChild(head);
    sec.appendChild(body);
    return sec;
  }

  // ─── Tab: Strategies ───────────────────────────────────────
  function renderStrategies(data, portfolio) {
    const list = (data && data.strategies) || [];
    if (S.sub && S.sub.strategy) {
      const match = list.find((x) => x.id === S.sub.strategy);
      return renderStrategyDetail(S.sub.strategy, match, portfolio);
    }
    const root = el("div");
    if (list.length === 0) {
      root.appendChild(section("Strategies",
        el("div", { class: "empty", html:
          "no strategies registered yet — see <code>docs://strategy-bundle</code> to author one" })));
      return root;
    }
    const tbl = el("table", { class: "t" });
    tbl.appendChild(el("thead", null, [el("tr", null, [
      el("th", { text: "id" }),
      el("th", { text: "name" }),
      el("th", { text: "triggers" }),
      el("th", { class: "num", text: "runs 24h" }),
      el("th", { class: "num", text: "ok" }),
      el("th", { class: "num", text: "fail" }),
      el("th", { text: "policy" }),
      el("th", { text: "last fire" }),
    ])]));
    const tbody = el("tbody");
    list.forEach((s) => {
      const tr = el("tr", { class: "click",
        onclick: () => { location.hash = "strategies?strategy=" + encodeURIComponent(s.id); },
      });
      tr.appendChild(el("td", { class: "mono", text: fmt.shortHex(s.id, 6, 4), title: s.id }));
      tr.appendChild(el("td", { text: s.name || "" }));
      tr.appendChild(el("td", { class: "mono",
        text: (s.trigger_kinds && s.trigger_kinds.length) ? s.trigger_kinds.join(", ") : "—" }));
      const last24 = s.last_24h || {};
      tr.appendChild(el("td", { class: "num mono", text: String(last24.runs != null ? last24.runs : 0) }));
      tr.appendChild(el("td", { class: "num mono", text: String(last24.succeeded || 0) }));
      tr.appendChild(el("td", { class: "num mono", text: String(last24.failed || 0) }));
      const polTd = el("td");
      polTd.appendChild(verdictBadge(s.policy_alignment));
      // Non-satisfied verdicts get a "copy report" button so the user can
      // paste the diagnosis context to the agent in one shot. The list
      // row only carries the verdict string; full alignment lives on
      // strategy://{id} (referenced in the report).
      if (s.policy_alignment && s.policy_alignment !== "satisfied") {
        polTd.appendChild(document.createTextNode(" "));
        polTd.appendChild(reportBtn("policy_alignment", {
          strategy_id:        s.id,
          name:               s.name,
          verdict:            s.policy_alignment,
          detail_uri:         "strategy://" + s.id,
          contracts_touched:  "see strategy://" + s.id + " for contracts_touched + missing entries",
        }));
      }
      tr.appendChild(polTd);
      tr.appendChild(el("td", { class: "mono", title: s.last_fire_at || "",
        text: s.last_fire_at ? fmt.rel(s.last_fire_at) : "—" }));
      tbody.appendChild(tr);
    });
    tbl.appendChild(tbody);
    const body = el("div", { class: "section-body flush" });
    body.appendChild(tbl);
    const head = el("div", { class: "section-head" }, [
      el("span", { text: "Strategies" }),
      el("span", { class: "mono dim", text: list.length + " active" }),
    ]);
    const sec = el("div", { class: "section" });
    sec.appendChild(head); sec.appendChild(body);
    const root2 = el("div"); root2.appendChild(sec);
    return root2;
  }

  function renderStrategyDetail(id, summary, portfolio) {
    const root = el("div");
    const back = el("div", { class: "section" });
    back.appendChild(el("div", { class: "section-head" }, [
      el("a", { href: "#strategies", text: "← back to strategies" }),
      el("span", { class: "mono dim", text: id }),
    ]));
    root.appendChild(back);

    // Read the per-strategy detail SYNCHRONOUSLY from cache (populated by
    // pollOnce). This avoids the renderTab/getJson race that left the
    // page stuck on "loading…" when a fresh poll overlapped the async
    // resolution — and removes a layer of DOM churn from the 5s tick.
    const d = S.cache.detail && S.cache.detail[id];
    const detailSec = el("div", { class: "section" });
    detailSec.appendChild(el("div", { class: "section-head", text: "detail" }));
    const detailBody = el("div", { class: "section-body" });
    detailSec.appendChild(detailBody);
    root.appendChild(detailSec);

    // Per-strategy triggers section, read sync from S.cache.triggers.
    // Match the same table shape (`.t` class + section flush-body + count
    // pill) as the Portfolio / Triggers tab so the look stays consistent
    // across surfaces.
    const all = (S.cache.triggers && S.cache.triggers.triggers) || [];
    // v1.8 lineage: a trigger may have been registered against a PRIOR
    // version of this strategy. Match by lineage_id when both sides know
    // it; fall back to strategy_id for legacy rows.
    const lineageId = (d && d.lineage_id) || null;
    const mine = all.filter((t) =>
      (lineageId && t.strategy_lineage_id === lineageId) ||
      t.strategy_id === id);
    const triggersSec = el("div", { class: "section" });
    triggersSec.appendChild(el("div", { class: "section-head" }, [
      el("span", { text: "triggers" }),
      el("span", { class: "mono dim",
        text: mine.length > 0 ? (mine.length + " attached") :
              (S.cache.triggers ? "none attached" : "loading…") }),
    ]));
    if (mine.length === 0) {
      const body = el("div", { class: "section-body" });
      body.appendChild(el("div", { class: "dim",
        text: S.cache.triggers ? "no triggers attached" : "loading…" }));
      triggersSec.appendChild(body);
    } else {
      const body = el("div", { class: "section-body flush" });
      const tbl = el("table", { class: "t" });
      tbl.appendChild(el("thead", null, [el("tr", null, [
        el("th", { text: "kind" }),
        el("th", { text: "note" }),
        el("th", { text: "enabled" }),
        el("th", { text: "last fired" }),
        el("th", { text: "id" }),
      ])]));
      const tb = el("tbody");
      mine.forEach((t) => {
        const tr = el("tr");
        tr.appendChild(el("td", { class: "mono", text: t.kind || "—" }));
        tr.appendChild(el("td", { class: t.note ? "" : "dim",
          text: t.note || "—", title: t.note || "" }));
        tr.appendChild(el("td", null, [
          t.enabled === false
            ? el("span", { class: "badge", text: "disabled" })
            : el("span", { class: "badge ok", text: "enabled" }),
        ]));
        tr.appendChild(el("td", { class: "mono", title: t.last_fired_at || "",
          text: t.last_fired_at ? fmt.rel(t.last_fired_at) : "—" }));
        tr.appendChild(el("td", { class: "mono",
          text: fmt.shortHex(t.id || "", 6, 4), title: t.id || "" }));
        tb.appendChild(tr);
      });
      tbl.appendChild(tb);
      body.appendChild(tbl);
      triggersSec.appendChild(body);
    }
    root.appendChild(triggersSec);

    // v1.10 actions: named one-shot helpers the bundle exposes. Rendered
    // as chips so an operator can see at a glance what `strategy_run(...,
    // action: "...")` will accept. Empty bundles skip the section entirely
    // so the legacy single-execute layout stays clean.
    if (d && Array.isArray(d.actions) && d.actions.length > 0) {
      const actionsSec = el("div", { class: "section" });
      actionsSec.appendChild(el("div", { class: "section-head" }, [
        el("span", { text: "named actions" }),
        el("span", { class: "mono dim", text: d.actions.length + " declared" }),
      ]));
      const body = el("div", { class: "section-body" });
      const chips = el("div", { class: "chips" });
      d.actions.forEach((name) => {
        chips.appendChild(el("span", { class: "chip mono", text: name, title:
          'manual one-shot: strategy_run({strategy_id, action: "' + name + '"})' }));
      });
      body.appendChild(chips);
      body.appendChild(el("div", { class: "dim", style: "margin-top:8px",
        text: "triggers cannot pick named actions — manual invocation only." }));
      actionsSec.appendChild(body);
      root.appendChild(actionsSec);
    }

    // Cache miss on first navigation — render placeholder; pollOnce will
    // populate the cache on its next tick and the resulting render will
    // pick up the data.
    if (!d) {
      detailBody.appendChild(el("div", { class: "dim", text: "loading…" }));
      return root;
    }

    // Direct sync render of the detail payload.
    ((d) => {
      const chain = portfolio && portfolio.chain_id;
      // meta block — `policy_alignment` is lifted out and rendered as a
      // dedicated row below so the verdict + copy-report button are the
      // primary affordance (instead of buried in an auto-rendered nested
      // object that duplicates the same fields three times).
      const meta = {};
      ["name", "description", "tags", "created_at", "deleted_at",
       "trigger_kinds", "last_fire_at", "has_bundle", "view_uri"]
        .forEach((k) => { if (d[k] != null) meta[k] = d[k]; });
      const kv = renderObjectAsKV(meta, chain);
      detailBody.appendChild(kv);

      // Inline policy_alignment as ANOTHER row in the same KV grid so the
      // label/value column alignment stays consistent. Value cell carries
      // the verdict badge + (when non-satisfied) the copy-report button +
      // inline remediation hint.
      const pa = d.policy_alignment;
      if (pa && typeof pa === "object" && pa.verdict) {
        kv.appendChild(el("div", { class: "k", text: "policy_alignment" }));
        const vCell = el("div", { class: "v" });
        const row = el("div", { class: "row gap" });
        row.appendChild(verdictBadge(pa.verdict));
        if (pa.verdict !== "satisfied") {
          const missingDesc = (pa.missing || []).map((m) => {
            const sels = (m.selectors || []).join(", ");
            return (m.contract || "?") + (sels ? " [" + sels + "]" : "") +
                   (m.reason ? " — " + m.reason : "");
          });
          row.appendChild(reportBtn("policy_alignment", {
            strategy_id:  d.strategy_id || id,
            name:         d.name,
            verdict:      pa.verdict,
            missing:      missingDesc,
            remediation:  pa.remediation,
            detail_uri:   "strategy://" + (d.strategy_id || id),
          }));
        }
        vCell.appendChild(row);
        if (pa.verdict !== "satisfied" && pa.remediation) {
          vCell.appendChild(el("div", { class: "dim small", text: pa.remediation }));
        }
        kv.appendChild(vCell);
      }

      // view auto-render
      const view = d.view_output || {};
      const data = view.data || view;
      const obs = {};
      if (data && typeof data === "object" && !Array.isArray(data)) {
        Object.keys(data).forEach((k) => { if (k !== "$assets") obs[k] = data[k]; });
      }
      // Render the View-output section when either there's data to show OR
      // the view failed (so the copy-report button is reachable even on a
      // bare confidence:"partial"/"missing" envelope with null data).
      const viewFailed = view.confidence && view.confidence !== "full";
      if (Object.keys(obs).length > 0 || viewFailed) {
        root.appendChild(section("View output", (function () {
          const b = el("div", { class: "section-body" });
          if (viewFailed) {
            const headRow = el("div", { class: "row gap" }, [
              el("span", { class: "badge partial", text: view.confidence }),
              reportBtn("view_failure", {
                strategy_id:  d.strategy_id || id,
                name:         d.name,
                confidence:   view.confidence,
                reason:       view.reason,
                remediation:  view.remediation,
                view_uri:     "strategy://" + (d.strategy_id || id) + "/view",
              }),
            ]);
            b.appendChild(headRow);
          }
          if (view.reason) b.appendChild(el("div", { class: "dim", text: view.reason }));
          if (Object.keys(obs).length > 0) b.appendChild(renderObjectAsKV(obs, chain));
          return b;
        })()));
      }
      // records — open by default now that we have a focused table view.
      // The generic renderValue spilled every column (including the
      // 64-char strategy_id), making each payload field stack vertically
      // one character at a time. The custom renderRecordsTable collapses
      // the row to when / record / summary / run / tx with an expandable
      // payload detail row.
      const records = (d.records && d.records.records) || [];
      if (records.length > 0) {
        const sk = "records:" + id;
        const open = S.expanded.has(sk) || !S.expanded.has(sk + ":closed");
        const head = el("div", { class: "section-head" }, [
          el("span", { class: "disclose",
            onclick: (ev) => {
              ev.preventDefault();
              // We invert the semantics so the default is OPEN (presence
              // of `${sk}:closed` means user explicitly collapsed it).
              if (open) {
                S.expanded.add(sk + ":closed");
                S.expanded.delete(sk);
              } else {
                S.expanded.delete(sk + ":closed");
                S.expanded.add(sk);
              }
              renderTab();
            },
            text: (open ? "▾" : "▸") + " records (" + records.length + ")" }),
          el("span", { class: "mono dim",
            text: "click a row to expand its full payload" }),
        ]);
        const body2 = el("div", { class: "section-body flush" });
        if (open) body2.appendChild(renderRecordsTable(records, chain));
        const sec = el("div", { class: "section" });
        sec.appendChild(head); sec.appendChild(body2);
        root.appendChild(sec);
      }
    })(d);

    return root;
  }

  // ─── Tab: Policy ───────────────────────────────────────────
  // v1.13 P4 — diff lens. A checkbox above the policy body toggles
  // between the standard renderObject layout (current revision only) and
  // a diff render against the immediately-previous revision. State is
  // persisted in localStorage so it survives reloads. The diff body is
  // fetched lazily via /api/policy/history?include_body=true&limit=2.
  const POLICY_DIFF_LS_KEY = "osmcp.policyDiffEnabled";
  function policyDiffEnabled() {
    try { return localStorage.getItem(POLICY_DIFF_LS_KEY) === "1"; }
    catch (e) { return false; }
  }
  function setPolicyDiffEnabled(v) {
    try { localStorage.setItem(POLICY_DIFF_LS_KEY, v ? "1" : "0"); }
    catch (e) {}
  }

  function renderPolicy(data) {
    const root = el("div");
    const current = (data && data.current) || {};
    const history = ((data && data.history) || {}).revisions || [];

    if (!current.loaded) {
      root.appendChild(section("Current policy",
        el("div", { class: "empty", html:
          (current.reason || "no policy installed") +
          (current.remediation ? "<br><span class='dim'>" + escapeHtml(current.remediation) + "</span>" : "") })));
    } else {
      const body = el("div", { class: "section-body" });
      body.appendChild(renderObjectAsKV({
        revision_id: current.revision_id,
        set_at: current.set_at,
        rationale: current.rationale,
      }, null));

      // v1.13 P4: diff toggle row. Checkbox state survives reloads.
      const diffOn = policyDiffEnabled();
      const toggle = el("label", { class: "diff-toggle" });
      const checkbox = el("input", { type: "checkbox", id: "policy-diff-toggle" });
      if (diffOn) checkbox.checked = true;
      checkbox.addEventListener("change", function () {
        setPolicyDiffEnabled(!!this.checked);
        // Clear the cached diff payload so the next pollOnce refetches
        // (or the manual reload below catches it immediately).
        S.cache.policyDiff = null;
        // Trigger a fresh render — pollOnce is async; renderTab is sync.
        renderTab();
        // Kick off the diff fetch if we're now on; otherwise leave alone.
        if (this.checked) ensurePolicyDiffLoaded();
      });
      toggle.appendChild(checkbox);
      toggle.appendChild(document.createTextNode(" show diff vs previous revision"));
      body.appendChild(toggle);

      // Body: either schema-aware single render OR diff render.
      // v1.13 P1: schema-aware policy body — no click-to-expand. Each
      // top-level key of `current.policy` (chains/contracts/selectors/
      // erc20_spend/native_value/raw_call — or whatever the backend ships
      // next) becomes its own panel. Backend may ship `_field_kinds` to
      // extend the value-formatter dispatch without a UI change (Track P2).
      if (!diffOn) {
        if (current.policy && typeof current.policy === "object") {
          body.appendChild(renderObject(current.policy, {
            field_kinds: current._field_kinds,
          }));
        } else {
          body.appendChild(el("div", { class: "muted", text: "(empty policy body)" }));
        }
      } else {
        // Diff mode — render whatever we have cached. The cache is
        // populated by ensurePolicyDiffLoaded() (kicked off in pollOnce
        // when the toggle is on, or on the change event above).
        body.appendChild(renderPolicyDiffBlock(current));
        ensurePolicyDiffLoaded();
      }

      root.appendChild(section("Current policy " +
        (current.revision_id ? "(rev " + current.revision_id + ")" : ""), body));
    }

    // History
    if (history.length === 0) {
      root.appendChild(section("History",
        el("div", { class: "empty", text: "no prior revisions" })));
    } else {
      const tbl = el("table", { class: "t" });
      tbl.appendChild(el("thead", null, [el("tr", null, [
        el("th", { text: "revision" }),
        el("th", { text: "set at" }),
        el("th", { text: "active" }),
        el("th", { text: "rationale" }),
      ])]));
      const tbody = el("tbody");
      history.forEach((r) => {
        const tr = el("tr");
        tr.appendChild(el("td", { class: "mono", text: r.revision_id || "—" }));
        tr.appendChild(el("td", { class: "mono", title: r.set_at, text: fmt.rel(r.set_at) }));
        tr.appendChild(el("td", null, [
          r.is_active ? el("span", { class: "badge ok", text: "active" })
                      : el("span", { class: "badge", text: "—" }),
        ]));
        tr.appendChild(el("td", { text: r.rationale || "" }));
        tbody.appendChild(tr);
      });
      tbl.appendChild(tbody);
      const body = el("div", { class: "section-body flush" });
      body.appendChild(tbl);
      root.appendChild(section("History (" + history.length + ")", body));
    }
    return root;
  }

  // v1.13 P4 — lazy fetch of /api/policy/history?include_body=true&limit=2.
  // Result lands in S.cache.policyDiff and triggers a re-render via
  // renderTab(). De-duplicated by an inflight flag so multiple renders
  // don't pile up requests.
  let _policyDiffInflight = false;
  async function ensurePolicyDiffLoaded() {
    if (S.cache.policyDiff) return;
    if (_policyDiffInflight) return;
    _policyDiffInflight = true;
    try {
      const r = await getJson("/api/policy/history?include_body=true&limit=2");
      S.cache.policyDiff = r;
    } catch (e) {
      S.cache.policyDiff = { _err: e && e.message ? e.message : String(e) };
    } finally {
      _policyDiffInflight = false;
      renderTab();
    }
  }

  // Build the diff-body block for the policy panel. Reads from
  // S.cache.policyDiff which is populated by ensurePolicyDiffLoaded.
  // Falls back to graceful banners when the data isn't ready, when
  // there's no previous revision, or when bodies failed to parse.
  function renderPolicyDiffBlock(current) {
    const wrap = el("div");
    const diffCache = S.cache.policyDiff;
    if (!diffCache) {
      wrap.appendChild(el("div", { class: "diff-banner", text: "Loading diff…" }));
      return wrap;
    }
    if (diffCache._err) {
      wrap.appendChild(el("div", { class: "diff-banner",
        text: "diff unavailable: " + diffCache._err }));
      // Still show current body so the page isn't empty.
      if (current.policy && typeof current.policy === "object") {
        wrap.appendChild(renderObject(current.policy, { field_kinds: current._field_kinds }));
      }
      return wrap;
    }
    const revisions = (diffCache.revisions || []);
    if (revisions.length < 2) {
      wrap.appendChild(el("div", { class: "diff-banner",
        text: "this is the initial revision; nothing to diff against" }));
      if (current.policy && typeof current.policy === "object") {
        wrap.appendChild(renderObject(current.policy, { field_kinds: current._field_kinds }));
      }
      return wrap;
    }
    // The history list is descending by set_at; the first entry is the
    // active (current) revision, the second is the immediately-previous.
    // Defensive: if `is_active` is set, prefer that as the curr anchor.
    const curr = revisions.find((r) => r.is_active) || revisions[0];
    const prev = revisions.find((r) => r.revision_id !== curr.revision_id) || revisions[1];
    if (!prev) {
      wrap.appendChild(el("div", { class: "diff-banner",
        text: "no previous revision available to diff against" }));
      if (current.policy && typeof current.policy === "object") {
        wrap.appendChild(renderObject(current.policy, { field_kinds: current._field_kinds }));
      }
      return wrap;
    }
    if (curr.body_parse_error || prev.body_parse_error) {
      const reason = curr.body_parse_error || prev.body_parse_error;
      wrap.appendChild(el("div", { class: "diff-banner",
        text: "diff unavailable: " + reason }));
      if (current.policy && typeof current.policy === "object") {
        wrap.appendChild(renderObject(current.policy, { field_kinds: current._field_kinds }));
      }
      return wrap;
    }

    // Diff header: prev → curr with set_at timestamps.
    const header = el("div", { class: "diff-header" });
    const short = (s) => s ? String(s).slice(0, 8) : "—";
    header.appendChild(el("span", { class: "mono",
      title: prev.set_at || "", text: short(prev.revision_id) + " @ " +
        (prev.set_at ? fmt.rel(prev.set_at) : "—") }));
    header.appendChild(el("span", { class: "diff-header-arrow", text: "→" }));
    header.appendChild(el("span", { class: "mono",
      title: curr.set_at || "", text: short(curr.revision_id) + " @ " +
        (curr.set_at ? fmt.rel(curr.set_at) : "—") }));
    wrap.appendChild(header);

    // Compute and render the diff. Reuse field_kinds from the cached
    // body if present (it travels inside `policy://history`'s body),
    // else fall back to current._field_kinds from the top-level cache.
    const fieldKinds = (diffCache._field_kinds) || (current && current._field_kinds) || null;
    const tree = diffJson(prev.body || {}, curr.body || {});
    wrap.appendChild(renderObjectDiff(tree, { field_kinds: fieldKinds }));
    return wrap;
  }

  // ─── Tab: Triggers ─────────────────────────────────────────
  function renderTriggers(data, strategiesData) {
    const list = (data && data.triggers) || [];
    if (list.length === 0) {
      const r = el("div");
      r.appendChild(section("Triggers",
        el("div", { class: "empty", text: "no triggers registered" })));
      return r;
    }
    // build a strategy_id → name map for attribution
    const stratMap = {};
    ((strategiesData && strategiesData.strategies) || []).forEach((s) => { stratMap[s.id] = s.name; });

    const tbl = el("table", { class: "t" });
    tbl.appendChild(el("thead", null, [el("tr", null, [
      el("th", { text: "kind" }),
      el("th", { text: "note" }),
      el("th", { text: "strategy" }),
      el("th", { text: "enabled" }),
      el("th", { text: "last fire" }),
      el("th", { text: "created" }),
      el("th", { text: "id" }),
    ])]));
    const tbody = el("tbody");
    list.forEach((t) => {
      const tr = el("tr");
      tr.appendChild(el("td", { class: "mono", text: t.kind || "—" }));
      tr.appendChild(el("td", { class: t.note ? "" : "dim",
        text: t.note || "—", title: t.note || "" }));
      const stratName = stratMap[t.strategy_id];
      const stratTd = el("td");
      if (stratName) {
        stratTd.appendChild(el("a", {
          href: "#strategies?strategy=" + encodeURIComponent(t.strategy_id),
          text: stratName,
        }));
        stratTd.appendChild(document.createTextNode(" "));
        stratTd.appendChild(el("span", { class: "mono dim", text: fmt.shortHex(t.strategy_id, 6, 4) }));
      } else {
        stratTd.appendChild(el("span", { class: "mono", text: fmt.shortHex(t.strategy_id, 6, 4) }));
      }
      tr.appendChild(stratTd);
      tr.appendChild(el("td", null, [
        t.enabled ? el("span", { class: "badge ok", text: "enabled" })
                  : el("span", { class: "badge", text: "disabled" }),
      ]));
      tr.appendChild(el("td", { class: "mono", title: t.last_fired_at || "",
        text: t.last_fired_at ? fmt.rel(t.last_fired_at) : "—" }));
      tr.appendChild(el("td", { class: "mono", title: t.created_at || "",
        text: t.created_at ? fmt.rel(t.created_at) : "—" }));
      tr.appendChild(el("td", { class: "mono", text: fmt.shortHex(t.id, 6, 4), title: t.id }));
      tbody.appendChild(tr);
    });
    tbl.appendChild(tbody);
    const body = el("div", { class: "section-body flush" });
    body.appendChild(tbl);
    const r = el("div");
    r.appendChild(section("Triggers (" + list.length + ")", body));
    return r;
  }

  // ─── Tab: History ──────────────────────────────────────────
  function renderHistory(data, strategiesData, portfolio) {
    const list = (data && data.runs) || [];
    const root = el("div");
    const stratMap = {};
    ((strategiesData && strategiesData.strategies) || []).forEach((s) => { stratMap[s.id] = s.name; });

    // Filter bar
    const bar = el("div", { class: "filterbar" });
    bar.appendChild(el("label", { text: "strategy" }));
    const sSel = el("select");
    sSel.appendChild(el("option", { value: "", text: "(all)" }));
    Object.keys(stratMap).forEach((id) => {
      const opt = el("option", { value: id, text: stratMap[id] || id });
      if (S.historyFilters.strategy_id === id) opt.setAttribute("selected", "");
      sSel.appendChild(opt);
    });
    sSel.addEventListener("change", () => {
      S.historyFilters.strategy_id = sSel.value;
      pollNow();
    });
    bar.appendChild(sSel);

    bar.appendChild(el("label", { text: "status" }));
    const stSel = el("select");
    ["", "succeeded", "failed", "noop"].forEach((v) => {
      const opt = el("option", { value: v, text: v || "(all)" });
      if (S.historyFilters.status === v) opt.setAttribute("selected", "");
      stSel.appendChild(opt);
    });
    stSel.addEventListener("change", () => {
      S.historyFilters.status = stSel.value;
      pollNow();
    });
    bar.appendChild(stSel);

    bar.appendChild(el("label", { text: "since" }));
    const sinceIn = el("input", { type: "text", placeholder: "RFC3339 (e.g. 2026-05-01T00:00:00Z)",
      value: S.historyFilters.since });
    sinceIn.addEventListener("change", () => {
      S.historyFilters.since = sinceIn.value.trim();
      pollNow();
    });
    bar.appendChild(sinceIn);

    if (list.length === 0) {
      const sec = el("div", { class: "section" });
      sec.appendChild(el("div", { class: "section-head" }, [el("span", { text: "History" }),
        el("span", { class: "mono dim", text: "0 runs" })]));
      sec.appendChild(bar);
      sec.appendChild(el("div", { class: "section-body" }, [
        el("div", { class: "empty", text: "no runs matching filter" })]));
      root.appendChild(sec);
      return root;
    }

    const tbl = el("table", { class: "t" });
    tbl.appendChild(el("thead", null, [el("tr", null, [
      el("th"),
      el("th", { text: "run id" }),
      el("th", { text: "strategy" }),
      el("th", { text: "entry" }),
      el("th", { text: "status" }),
      el("th", { class: "num", text: "actions" }),
      el("th", { text: "started" }),
      el("th", { text: "finished" }),
    ])]));
    const tbody = el("tbody");
    list.forEach((r) => {
      const sk = "run:" + r.run_id;
      const open = S.expanded.has(sk);
      const tr = el("tr", { class: "click" });
      const discTd = el("td", { class: "mono dim", text: open ? "▾" : "▸" });
      tr.appendChild(discTd);
      tr.appendChild(el("td", { class: "mono", text: fmt.shortHex(r.run_id, 6, 4), title: r.run_id }));
      const stratName = stratMap[r.strategy_id] || fmt.shortHex(r.strategy_id, 6, 4);
      const stratTd = el("td");
      stratTd.appendChild(el("a", {
        href: "#strategies?strategy=" + encodeURIComponent(r.strategy_id),
        text: stratName,
      }));
      tr.appendChild(stratTd);
      // v1.10 entry-point column: "execute" for trigger / default runs,
      // the action name for manual `strategy_run({action: "..."})` calls.
      // Dim the execute label so the eye picks out the named-action rows.
      const entryLabel = r.action ? String(r.action) : "execute";
      const entryClass = r.action ? "mono" : "mono dim";
      tr.appendChild(el("td", { class: entryClass, text: entryLabel }));
      tr.appendChild(el("td", null, [statusBadge(r.status)]));
      tr.appendChild(el("td", { class: "num mono", text: String(r.action_count != null ? r.action_count : 0) }));
      tr.appendChild(el("td", { class: "mono", title: r.started_at || "",
        text: r.started_at ? fmt.rel(r.started_at) : "—" }));
      tr.appendChild(el("td", { class: "mono", title: r.finished_at || "",
        text: r.finished_at ? fmt.rel(r.finished_at) : "—" }));
      tr.addEventListener("click", (ev) => {
        if (ev.target && ev.target.tagName === "A") return; // don't expand on link clicks
        if (S.expanded.has(sk)) S.expanded.delete(sk); else S.expanded.add(sk);
        renderTab();
      });
      tbody.appendChild(tr);
      if (open) {
        const detailRow = el("tr");
        const td = el("td", { colspan: 8 });
        const inner = el("div", { class: "nested" });
        inner.appendChild(el("div", { class: "dim", text: "loading run detail…" }));
        td.appendChild(inner);
        detailRow.appendChild(td);
        tbody.appendChild(detailRow);
        getJson("/api/run/" + encodeURIComponent(r.run_id)).then((d) => {
          inner.innerHTML = "";
          const chain = portfolio && portfolio.chain_id;
          if (d.execution) inner.appendChild(section("execution", (function () {
            const b = el("div", { class: "section-body" });
            b.appendChild(renderValue(d.execution, "execution", chain));
            return b;
          })()));
          if (d.journal) inner.appendChild(section("journal", (function () {
            const b = el("div", { class: "section-body" });
            b.appendChild(renderValue(d.journal, "journal", chain));
            return b;
          })()));
        }).catch((e) => {
          inner.innerHTML = "";
          inner.appendChild(el("div", { class: "dim", text: "failed to load run: " + e.message }));
        });
      }
    });
    tbl.appendChild(tbody);

    const sec = el("div", { class: "section" });
    sec.appendChild(el("div", { class: "section-head" }, [el("span", { text: "History" }),
      el("span", { class: "mono dim", text: list.length + " runs" })]));
    sec.appendChild(bar);
    const sb = el("div", { class: "section-body flush" });
    sb.appendChild(tbl);
    sec.appendChild(sb);
    root.appendChild(sec);
    return root;
  }

  function statusBadge(s) {
    const m = {
      succeeded: "ok", failed: "bad", simulation_denied: "bad",
      policy_denied: "bad", canceled: "warn", started: "partial",
      running: "partial",
    };
    return el("span", { class: "badge " + (m[s] || ""), text: s || "—" });
  }

  function escapeHtml(s) {
    return String(s == null ? "" : s)
      .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;").replace(/'/g, "&#39;");
  }

  // ─── Fragment router ───────────────────────────────────────
  function parseHash() {
    const h = (location.hash || "#portfolio").replace(/^#/, "");
    const [name, query] = h.split("?", 2);
    const tab = ["portfolio", "strategies", "policy", "triggers", "history"]
      .includes(name) ? name : "portfolio";
    const sub = {};
    if (query) {
      query.split("&").forEach((p) => {
        const [k, v] = p.split("=", 2);
        sub[decodeURIComponent(k)] = decodeURIComponent(v || "");
      });
    }
    return { tab, sub };
  }

  function setActiveTab() {
    $$(".tab").forEach((t) => {
      t.classList.toggle("active", t.dataset.tab === S.tab);
    });
  }

  function renderTab() {
    const root = $("#content");
    root.innerHTML = "";
    let view;
    try {
      switch (S.tab) {
        case "portfolio":
          view = S.cache.portfolio
            ? renderPortfolio(S.cache.portfolio)
            : el("div", { class: "empty", text: "loading…" });
          break;
        case "strategies":
          view = renderStrategies(S.cache.strategies || { strategies: [] }, S.cache.portfolio);
          break;
        case "policy":
          view = S.cache.policy
            ? renderPolicy(S.cache.policy)
            : el("div", { class: "empty", text: "loading…" });
          break;
        case "triggers":
          view = renderTriggers(S.cache.triggers || { triggers: [] }, S.cache.strategies);
          break;
        case "history":
          view = renderHistory(S.cache.runs || { runs: [] }, S.cache.strategies, S.cache.portfolio);
          break;
        default:
          view = el("div", { class: "empty", text: "unknown tab" });
      }
    } catch (e) {
      view = el("div", { class: "empty",
        text: "render error: " + (e && e.message ? e.message : String(e)) });
    }
    root.appendChild(view);
  }

  // ─── Polling ───────────────────────────────────────────────
  function buildRunsPath() {
    const f = S.historyFilters;
    const qs = [];
    if (f.strategy_id) qs.push("strategy_id=" + encodeURIComponent(f.strategy_id));
    if (f.status)      qs.push("status="      + encodeURIComponent(f.status));
    if (f.since)       qs.push("since="       + encodeURIComponent(f.since));
    return "/api/runs" + (qs.length ? "?" + qs.join("&") : "");
  }

  async function pollOnce() {
    if (S.inflight) return;
    S.inflight = true;
    try {
      // Portfolio + strategies always fetched (the strategies map is used
      // by Triggers and History for attribution).
      const tasks = [
        getJson("/api/portfolio").catch((e) => ({ _err: e })),
        getJson("/api/strategies").catch((e) => ({ _err: e })),
      ];
      // Tab-specific extras. The strategy DETAIL view (strategies tab with
      // a sub.strategy) also wants live triggers AND the per-strategy
      // detail payload (records, view_output, policy_alignment, etc.) —
      // we cache that under S.cache.detail[id] so renderStrategyDetail
      // can read it synchronously, avoiding the render/fetch race that
      // dropped the View output on every other poll.
      if (S.tab === "policy")   tasks.push(getJson("/api/policy").catch((e) => ({ _err: e })));
      if (S.tab === "triggers") tasks.push(getJson("/api/triggers").catch((e) => ({ _err: e })));
      if (S.tab === "history")  tasks.push(getJson(buildRunsPath()).catch((e) => ({ _err: e })));
      const onDetail = S.tab === "strategies" && S.sub && S.sub.strategy;
      if (onDetail) {
        tasks.push(getJson("/api/triggers").catch((e) => ({ _err: e })));
        tasks.push(getJson("/api/strategy/" + encodeURIComponent(S.sub.strategy))
          .catch((e) => ({ _err: e })));
      }

      const results = await Promise.all(tasks);
      if (!results[0]._err) S.cache.portfolio  = results[0];
      if (!results[1]._err) S.cache.strategies = results[1];
      const tail = results.slice(2);
      if (S.tab === "policy"   && tail[0] && !tail[0]._err) S.cache.policy   = tail[0];
      // v1.13 P4: kick off the diff fetch only when the user has the
      // toggle on. Result lands in S.cache.policyDiff and triggers its
      // own renderTab() when complete.
      if (S.tab === "policy" && policyDiffEnabled() && !S.cache.policyDiff) {
        ensurePolicyDiffLoaded();
      }
      if (S.tab === "triggers" && tail[0] && !tail[0]._err) S.cache.triggers = tail[0];
      if (S.tab === "history"  && tail[0] && !tail[0]._err) S.cache.runs     = tail[0];
      if (onDetail) {
        if (tail[0] && !tail[0]._err) S.cache.triggers = tail[0];
        if (tail[1] && !tail[1]._err) {
          S.cache.detail = S.cache.detail || {};
          S.cache.detail[S.sub.strategy] = tail[1];
        }
      }

      // Consider the cycle successful if at least portfolio came through;
      // others are best-effort and the previous cache stays valid.
      if (!results[0]._err) {
        S.lastOk = Date.now();
        S.lastErr = null;
      } else {
        S.lastErr = results[0]._err;
      }
      updateHeader(S.cache.portfolio);
      // Anti-flicker: hash the inputs the current tab consumes; if the
      // payload is unchanged since the last render, skip the rebuild
      // entirely. The DOM stays exactly as it was — no flash.
      const fingerprint = jsonHash(currentTabFingerprint());
      if (fingerprint !== S.lastTabHash) {
        S.lastTabHash = fingerprint;
        renderTab();
      }
    } finally {
      S.inflight = false;
    }
  }

  // Compute the subset of S.cache + S.sub that the current tab actually
  // renders against. Two polls producing identical fingerprints can
  // safely skip the renderTab() rebuild — same data, same DOM.
  function currentTabFingerprint() {
    switch (S.tab) {
      case "portfolio":
        return { t: "portfolio", p: S.cache.portfolio };
      case "strategies": {
        const sid = S.sub && S.sub.strategy;
        if (sid) {
          return {
            t: "detail", sid,
            d: S.cache.detail && S.cache.detail[sid],
            tr: S.cache.triggers,
            // burner / chain context only — full portfolio churns too much
            chain: S.cache.portfolio && S.cache.portfolio.chain_id,
          };
        }
        return { t: "strategies", s: S.cache.strategies, p: S.cache.portfolio };
      }
      case "policy":
        return { t: "policy", p: S.cache.policy,
                 diffOn: policyDiffEnabled(), pd: S.cache.policyDiff };
      case "triggers":
        return { t: "triggers", g: S.cache.triggers, s: S.cache.strategies };
      case "history":
        return { t: "history", r: S.cache.runs, s: S.cache.strategies };
      default:
        return { t: S.tab };
    }
  }

  function pollNow() { pollOnce(); }

  function startPoller() {
    stopPoller();
    pollOnce();
    S.poller = setInterval(() => {
      if (document.visibilityState === "visible") pollOnce();
    }, POLL_MS);
  }

  function stopPoller() {
    if (S.poller) { clearInterval(S.poller); S.poller = null; }
  }

  // ─── Init ──────────────────────────────────────────────────
  function applyHash() {
    const { tab, sub } = parseHash();
    S.tab = tab;
    S.sub = sub && Object.keys(sub).length ? sub : null;
    // Reset the tab-level anti-flicker hash on navigation so the next
    // poll always re-renders against the new tab/sub (rather than
    // being silently skipped against a stale fingerprint).
    S.lastTabHash = "";
    setActiveTab();
    renderTab();
    pollNow();
  }

  window.addEventListener("hashchange", applyHash);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") pollNow();
  });

  // Default tab if no hash
  if (!location.hash) location.hash = "#portfolio";
  setActiveTab();
  applyHash();
  startPoller();
})();

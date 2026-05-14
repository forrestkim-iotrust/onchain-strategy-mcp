# site/

Static homepage for `onchain-strategy-mcp`. No build step. Pure HTML + one CSS file.

## Local preview

```bash
open site/index.html        # macOS — opens in default browser via file://
# or
python3 -m http.server -d site 8000 && open http://localhost:8000
```

## Deploy (GitHub Pages)

1. GitHub → **Settings → Pages**
2. **Source**: `Deploy from a branch`
3. **Branch**: `main`, **Folder**: `/site`
4. Save. Live at:
   `https://forrestkim-iotrust.github.io/onchain-strategy-mcp/`
   (Korean: `.../onchain-strategy-mcp/ko/`)

## Custom domain (optional, later)

Rename `CNAME.example` → `CNAME` and put a single line with your domain
(e.g. `onchain-strategy.dev`). Then point a CNAME DNS record at
`forrestkim-iotrust.github.io`. Not configured yet.

## Files

```
site/
├── index.html        # English homepage
├── ko/index.html     # Korean homepage (mirror)
├── styles.css        # Shared stylesheet
├── CNAME.example     # Placeholder for future custom domain
└── README.md         # This file
```

Total page weight: ~50 KB per page (HTML + CSS, no external assets).

# Fonts for the people page

The page the engine's `run` serves for `human` neurons ([decision 0008](../../docs/decisions/0008-human-neurons-on-a-page.md))
wears kevinferron.com's look. Its two fonts are served from the page's own origin at `/fonts/<name>`, behind the
page's token (`src/engine/page.ts`); the page fetches nothing from anywhere else.

| File | Family | Licence |
|---|---|---|
| `JosefinSans-wght.ttf` | Josefin Sans (variable, weight 100 to 700) | SIL OFL 1.1, **Reserved Font Name "Josefin Sans"** (`OFL-JosefinSans.txt`) |
| `PixelifySans-wght.ttf` | Pixelify Sans (variable, weight 400 to 700) | SIL OFL 1.1, no Reserved Font Name (`OFL-PixelifySans.txt`) |

Both are the unmodified upstream bytes from github.com/google/fonts (`ofl/josefinsans/JosefinSans[wght].ttf`,
`ofl/pixelifysans/PixelifySans[wght].ttf`); only the file names differ. Josefin Sans carries a Reserved Font Name, so
it must not be subset, converted or edited here. The fonts are under the OFL, not the repo's MIT licence.

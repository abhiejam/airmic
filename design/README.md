# Design

Source files exported from the Claude Design canvases. The canvases are the living versions; these copies are snapshots (2026-10-03).

| Folder | Canvas | Contents |
|---|---|---|
| `mockups/` | [AirMic iPhone mockups](https://claude.ai/artifact/SFuouTH2921fG9QzJidshH) | Home (streaming, muted, no computer), Connect, Summary, light and dark |
| `logo/` | [AirMic logo concepts](https://claude.ai/artifact/J6CQZq1NW4ka29idedE5Ga) | 7 concepts + OnAirMic.com lockups; **chosen: 1 · Signal** (`Main.dc.html`) |

## Using these files
- `*.dc.html` files are Claude Design components: HTML plus `{{…}}` template holes, run by the canvas runtime (`support.js`, not included). They don't render on their own in a browser; open the canvas links above to view them. They are the reference for exact colors, sizes and copy.
- `canvas.json` is each canvas's layout index.
- `logo/airmic-mark.svg`: the Signal mark, indigo stroke, transparent background. Standalone, use anywhere.
- `logo/airmic-app-icon.svg`: 1024×1024 iOS app icon (full-bleed indigo square, white mark; iOS rounds the corners). Export to PNG for the Xcode asset catalog.

## Design tokens (from the mockups)

| Token | Light | Dark |
|---|---|---|
| Background | `#F6F2EA` | `#111114` |
| Surface | `#FFFDF8` | `#1B1B20` |
| Text | `#1B1A22` | `#F3F0E8` |
| Secondary text | `#66646E` | `#A3A1AB` |
| Line | `#E6E0D3` | `#2C2C33` |
| Accent | `#5146E5` | `#857EED` (accent mixed 30% with white) |
| Muted / warning | `#C2410C` | `#F0894A` |
| Live dot | `#2F8F5B` | `#4CC38A` |

Font: SF Pro (system). Primary buttons 56 px tall, fully rounded, 280 px wide when centred. Screen padding 60 / 24 / 40 px (top / sides / bottom).

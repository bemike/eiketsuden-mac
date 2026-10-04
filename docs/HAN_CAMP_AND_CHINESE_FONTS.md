# 0.1.7: Chinese menu glyphs and Han-style camp

The previous Chinese font completion scanned the old source tree, missing later UI additions. Both fonts now include 么宫报给贩领. In particular 贩卖 and 武将情报 render without missing glyphs. Existing glyph artwork and licences are preserved.

The camp backdrop is an AI-generated late Eastern Han-style illustration, not a historical artifact or original DOS artwork. It uses rectangular military tents, timber fortifications and cross-collared/lamellar-clad soldiers rather than a Qing imperial banquet. It is intentionally a stylized game setting, not an archaeological reconstruction.

Master and pipeline input: tools/assets/custom/han_camp.png
Game asset: data/base/gfx/bg/camp.png
Prompt: tools/assets/custom/han_camp_prompt.txt

Font donor archives (Fusion Pixel v2026.09.01, OFL):
https://github.com/TakWolf/fusion-pixel-font/releases/download/2026.09.01/fusion-pixel-font-12px-proportional-ttf.woff2-v2026.09.01.zip
https://github.com/TakWolf/fusion-pixel-font/releases/download/2026.09.01/fusion-pixel-font-10px-proportional-ttf.woff2-v2026.09.01.zip

Audit a packaged build, not only the base data, to include translated campaign metadata:

```sh
python tools/assets/verify_chinese_fonts.py --data-root '/path/to/app/Contents/Resources/data'
```

The audit fails on any missing CJK character or empty CJK outline in either font. Its source scan conservatively includes comments as well as UI strings.

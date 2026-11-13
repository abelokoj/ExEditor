# Dictionary source assets (work in progress)

These eight optional data archives have been collected locally for the planned ExEditor dictionary downloader. They contain seven Hunspell dictionaries (US/UK English, French, German, Spanish, Brazilian/European Portuguese) and Finnish Voikko data, with upstream notices and licenses. The Voikko archive also includes its corresponding source. No LibreOffice application or Android runtime is included.

The download interface, generated catalog and ten planned Divvun archives are not yet complete. These files are not included in the installer and are not automatically downloaded by the current app. `scripts/collect-dictionary-assets.py` records the pinned upstream sources and can complete collection; it requires Python and zstandard and does not compile ExEditor or a dictionary.

## Collected archives

- `hunspell-de-de-libreoffice-c0865a85eb67.zip` — 1,265,406 bytes; SHA-256 `b5d6489b8c4024b411a9eeafbee1b716f8bdbc5f926afb6d6e949241e1e0ac40`
- `hunspell-en-gb-libreoffice-c0865a85eb67.zip` — 479,938 bytes; SHA-256 `7bae700bbab2f3a79b72d2431fa26573b373c188185693a5e73c98b84081041a`
- `hunspell-en-us-libreoffice-c0865a85eb67.zip` — 206,277 bytes; SHA-256 `714eb8c9238119555b66c983201b83ce007994931bdad6f0dc81a0f59f53b04c`
- `hunspell-es-es-libreoffice-c0865a85eb67.zip` — 271,799 bytes; SHA-256 `46b4d6ba43cb80487b8304599294c2877ab51b76b2bc8231dae0457b08c2338c`
- `hunspell-fr-fr-libreoffice-c0865a85eb67.zip` — 347,241 bytes; SHA-256 `13b85f73532510a4ed11d3245daf41d70228932adbe01c6f918d7300c7bdbe2d`
- `hunspell-pt-br-libreoffice-c0865a85eb67.zip` — 1,424,488 bytes; SHA-256 `b7bf547a2f87d16e1a345c0e1ed63bf8e228970b2f9c64dcabfd58a3d7a473fa`
- `hunspell-pt-pt-libreoffice-c0865a85eb67.zip` — 244,156 bytes; SHA-256 `8cc45df7eefc373239132d152405c2f52d9c29c5ac81fc61536f532f1d0b2837`
- `voikko-fi-2.6.zip` — 2,376,711 bytes; SHA-256 `4f50255dffbe2e97b104652b852b6e86ea3e567c63c413487c6f5b2c0f8262bc`

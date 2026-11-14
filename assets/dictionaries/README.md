# Optional spelling dictionary data

These optional archives are downloaded through ExEditor's Language Tools; this branch's integration is awaiting GitHub validation. They are never embedded in the application or installer. Each contains unmodified dictionary files, notices and license texts. Large ZIPs are stored in ordered 50 MiB parts, with both per-part and complete-archive hashes in the catalog. Compiled Voikko/Divvun packs also include corresponding source archives. No LibreOffice application or Android library is included.

The common Hunspell pairs come from LibreOffice/dictionaries at c0865a85eb67dc377f2cf8e8fe2c4fa9e53f0dcb. Finnish Voikko and the catalog of ten released GiellaLT spellers follow WM Keyboard at 370d1e326427cdf98c6a777e3d72bfb0c27b5d27 and its data repository at 2c05d4a419a983724ca2479c87f04e712bc5559d. Three versioned GiellaLT pre-releases are marked as preview in the catalog.

Runtime downloads use the pinned ExEditor dictionary-data commit in the catalog and per-file/ archive SHA-256 and size checks. Until these local files are committed and pushed, those ExEditor-hosted download links are not published.

Run scripts/collect-dictionary-assets.py only to collect/repackage existing data; it does not compile a dictionary, engine or app. It requires Python and zstandard. The compiled-in catalog is assets/accessories/dictionaries.json. Its hashes cover every installed file, including source and license material.

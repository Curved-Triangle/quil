# Benchmark corpus provenance

The full local benchmark corpus uses **Mini LibriSpeech, OpenSLR SLR31, `dev-clean-2`**:

- Official page: <https://www.openslr.org/31>
- Archive: <https://openslr.trmal.net/resources/31/dev-clean-2.tar.gz>
- Archive size: 126,046,265 bytes
- Official MD5: `6d7ab67ac6a1d2c993d050e16d61080d`
- SHA-256 recorded after verifying that MD5: `176ec501490eced2d6c1f89f4f0ddc7dfe799e649e5322f8ba49fe3ff50c8012`
- License: **Creative Commons Attribution 4.0 International**. Attribution: “Mini LibriSpeech, OpenSLR SLR31.” License: <https://creativecommons.org/licenses/by/4.0/>

Corpus audio and generated WAV files remain under `benchmark-data/` and are not included in Quil releases. Any small redistributed regression fixture has its own attribution and license notice in `tests/fixtures/NOTICE.md`.

The benchmark corpus is not microphone data captured by Quil. Do not add user recordings to it.

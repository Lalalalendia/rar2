# Cloud Reader bundled open font replacements

This directory contains deterministic font resources shipped with the Cloud Reader release packet.

| Source family matched by Reader | Bundled resource | Upstream | SHA-256 |
| --- | --- | --- | --- |
| Calibri | Carlito Regular | googlefonts/carlito, `fonts/ttf/Carlito-Regular.ttf` | `f6418f708baede9789daef5d458c0f53d2a888af9820e8062934e504fedc6595` |
| Cambria | Caladea Regular | googlefonts/caladea, `fonts/ttf/Caladea-Regular.ttf` | `f1e899278b7b4491aba5b6a8253c4b04c050cc59b21865be5c37559a775153cd` |

The server still resolves by explicit `source_family` configuration. It does not inspect or discover ambient host fonts. Operator-provided exact resources for the same source family take precedence over the managed defaults.

Carlito and Caladea are redistributed under the SIL Open Font License 1.1; the corresponding license texts are included beside the font files.

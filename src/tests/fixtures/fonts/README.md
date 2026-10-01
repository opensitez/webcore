# Font fixtures

`bootstrap-icons-1.11.3.woff2` is the unmodified Bootstrap Icons 1.11.3 font:
https://cdn.jsdelivr.net/npm/bootstrap-icons@1.11.3/font/fonts/bootstrap-icons.woff2

License: `LICENSE-bootstrap-icons` (MIT).

The CSS tests embed this fixture to exercise WOFF2 reconstruction, font
registration, icon shaping without fallback, and visible glyph rasterization.
Tests do not require network access or a preexisting temporary download.

`roboto-v20-latin-regular.eot` is a MicroType Express-compressed Roboto Regular
web font fixture. Roboto is copyright The Roboto Project Authors and licensed
under Apache License 2.0; see `LICENSE-roboto-apache-2.0`. This fixture is used
only by tests to exercise real three-stream MTX decoding, CTF reconstruction,
incremental byte delivery, and fontdb registration without network access.

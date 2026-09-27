# Golden files

Reference drawings for the Rust renderer, which must reproduce them exactly. They were
generated on 2026-09-27 by the Python prototypes (`face_preview.py` and `rack_preview.py`,
`--plain`, default settings) and are only changed on purpose.

- `faces/<model>.txt`: each catalog model's face, with the sample name `srv01`.
  `.numbers` adds `--numbers`, `.off` adds `--state off`, and `.strip` marks a strip face.
- `rack/rack.txt`: the front view of `rack/rack.kdl` with `rack/wiring.kdl`.
- `.ansi` files hold the same drawings with their styles (colours, tints and the underline
  that separates devices), without `--plain`. View them with `cat`.

Trailing spaces are part of the drawings.

# webcore

HTML/CSS editor and renderer in Rust using `tiny-skia`.

`webcore` is a high-performance, lightweight library for parsing, styling, and rendering HTML and CSS directly in Rust. Built on top of `tiny-skia`, it provides a cross-platform foundation for rendering rich text, layouts, and web content in your native desktop applications, without the overhead of a full browser engine like Chromium.

## Features

- **Rendering Engine:** Fast, software-based rendering via `tiny-skia`.
- **Styling & Layout:** Core CSS cascade and layout support.
- **Interactivity:** Content editing (`contenteditable`), event handling, and transitions.
- **Cross-Platform Windowing:** Integration with `winit`.
- **Accessibility:** Built-in `accesskit` support for screen readers.

## Getting Started

Add `webcore` to your `Cargo.toml`:

```toml
[dependencies]
webcore = "0.3.3"
```

## Minimal Example

Here is a basic example of how to parse HTML and render it to a PNG image using `webcore`.

```rust
use webcore::{load_html_with_base, Renderer};
use tiny_skia::Pixmap;

fn main() {
    let html = "<h1>Hello WebCore!</h1><p>Rendering HTML in Rust.</p>";
    
    // Parse the HTML document
    let mut doc = load_html_with_base(html, "http://localhost/");
    
    // Setup the renderer and layout the document
    let mut renderer = Renderer::new(800, 600, 1.0);
    renderer.layout(&mut doc);
    
    // Create a pixel buffer and render to it
    let mut pixmap = Pixmap::new(800, 600).unwrap();
    renderer.paint(&doc, &mut pixmap.as_mut());
    
    // Save the output to a PNG file
    pixmap.save_png("output.png").unwrap();
}
```

## Examples

The repository includes several examples demonstrating the capabilities of `webcore`. You can run them using `cargo run --example <name>`:

- `animation_demo`: CSS animations
- `transitions_demo`: CSS transitions
- `contenteditable_demo`: Interactive text editing
- `markdown_demo`: Rendering markdown to HTML
- `print_demo`: Print layouts
- `layout_features`: Advanced CSS layout features
- `cascade_features`: CSS cascade rules
- `transform_filter_demo`: CSS transforms and filters
- `event_playground`: Interactive event handling

```bash
cargo run --example contenteditable_demo
```

## Copyright

Copyright (c) 2026 OpenSitez.com and Youness El Andaloussi. All rights reserved.

## License

This project is dual-licensed to offer both open-source and commercial use:

**1. Open Source (GPLv3)**
If you are building an open-source application under a compatible license, you may use `webcore` under the terms of the [GNU General Public License v3.0](LICENSE-GPL) (or later).

**2. Commercial License**
If you want to use `webcore` in a proprietary, closed-source commercial product without the requirements of the GPLv3, you must purchase a Commercial License. Please contact us for pricing and details.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dedicated to the public domain (or equivalent, such as the CC0 1.0 Universal public domain dedication). This ensures that all contributions can be freely incorporated into both the open-source (GPLv3) and commercial releases of the project without additional licensing restrictions.

//! Flexbox and Box Alignment compliance corpus.
//!
//! Each case is a whole document plus the geometry a real browser produces for
//! it — captured from Chrome, not written by hand — so a regression shows up as
//! a named element with two numbers rather than as a vague layout complaint.
//!
//! There is no WPT checkout in this tree, so this is not a conformance run. It
//! is a floor: the behaviour that has been checked against a browser stays
//! checked. Add a case whenever a spec question comes up, with the browser's
//! answer beside it.

use crate::Renderer;

/// One document and every id'd element's border box, as `(id, x, y, w, h)`.
struct Case {
    name: &'static str,
    /// The spec sections the case covers, for the reader who has to fix it.
    spec: &'static str,
    html: &'static str,
    expect: &'static [(&'static str, f32, f32, f32, f32)],
}

/// The viewport the expectations were captured at. Geometry is absolute, so
/// this has to match or every position shifts.
const VIEWPORT_W: f32 = 1280.0;
const VIEWPORT_H: f32 = 900.0;

const CASES: &[Case] = &[
    Case {
        name: "basis",
        spec: "§7.2 flex-basis, §9.7 resolving flexible lengths",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;width:300px;height:40px;background:#eee;margin-bottom:4px}\n.c > div{height:20px;background:#8cf}\n</style>\n<div class=c id=a><div id=a1 style=\"flex-basis:50%\"></div><div id=a2 style=\"flex-basis:25%\"></div></div>\n<div class=c id=b><div id=b1 style=\"flex:1 1 0\"></div><div id=b2 style=\"flex:2 1 0\"></div></div>\n<div class=c id=c><div id=c1 style=\"flex-basis:auto;width:100px\"></div><div id=c2 style=\"flex-basis:auto;width:50px\"></div></div>\n<div class=c id=d><div id=d1 style=\"flex:0 1 200px\"></div><div id=d2 style=\"flex:0 3 200px\"></div></div>\n<div class=c id=e><div id=e1 style=\"flex:0 1 200px;min-width:180px\"></div><div id=e2 style=\"flex:0 1 200px\"></div></div>\n<div class=c id=f><div id=f1 style=\"flex:1 1 0;max-width:50px\"></div><div id=f2 style=\"flex:1 1 0\"></div></div>\n<div class=c id=g style=\"gap:10px\"><div id=g1 style=\"flex:1\"></div><div id=g2 style=\"flex:1\"></div></div>\n<div class=c id=h><div id=h1 style=\"flex:1 1 auto;width:400px\"></div><div id=h2 style=\"flex:1 1 auto;width:400px\"></div></div>\n<div class=c id=i><div id=i1 style=\"flex-basis:0%;flex-grow:1\"></div><div id=i2 style=\"flex-basis:0%;flex-grow:3\"></div></div>\n<div class=c id=j><div id=j1 style=\"flex:none;width:120px\"></div><div id=j2 style=\"flex:auto;width:20px\"></div></div>\n<div class=c id=k style=\"flex-direction:column;height:200px;width:100px\"><div id=k1 style=\"flex-basis:25%\"></div><div id=k2 style=\"flex-basis:50%\"></div></div>\n<div class=c id=l><div id=l1 style=\"flex:0 1 400px;min-width:0\"></div><div id=l2 style=\"flex:0 1 100px;min-width:0\"></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 300.0, 40.0),
            ("a1", 0.0, 0.0, 150.0, 20.0),
            ("a2", 150.0, 0.0, 75.0, 20.0),
            ("b", 0.0, 44.0, 300.0, 40.0),
            ("b1", 0.0, 44.0, 100.0, 20.0),
            ("b2", 100.0, 44.0, 200.0, 20.0),
            ("c", 0.0, 88.0, 300.0, 40.0),
            ("c1", 0.0, 88.0, 100.0, 20.0),
            ("c2", 100.0, 88.0, 50.0, 20.0),
            ("d", 0.0, 132.0, 300.0, 40.0),
            ("d1", 0.0, 132.0, 175.0, 20.0),
            ("d2", 175.0, 132.0, 125.0, 20.0),
            ("e", 0.0, 176.0, 300.0, 40.0),
            ("e1", 0.0, 176.0, 180.0, 20.0),
            ("e2", 180.0, 176.0, 120.0, 20.0),
            ("f", 0.0, 220.0, 300.0, 40.0),
            ("f1", 0.0, 220.0, 50.0, 20.0),
            ("f2", 50.0, 220.0, 250.0, 20.0),
            ("g", 0.0, 264.0, 300.0, 40.0),
            ("g1", 0.0, 264.0, 145.0, 20.0),
            ("g2", 155.0, 264.0, 145.0, 20.0),
            ("h", 0.0, 308.0, 300.0, 40.0),
            ("h1", 0.0, 308.0, 150.0, 20.0),
            ("h2", 150.0, 308.0, 150.0, 20.0),
            ("i", 0.0, 352.0, 300.0, 40.0),
            ("i1", 0.0, 352.0, 75.0, 20.0),
            ("i2", 75.0, 352.0, 225.0, 20.0),
            ("j", 0.0, 396.0, 300.0, 40.0),
            ("j1", 0.0, 396.0, 120.0, 20.0),
            ("j2", 120.0, 396.0, 180.0, 20.0),
            ("k", 0.0, 440.0, 100.0, 200.0),
            ("k1", 0.0, 440.0, 100.0, 50.0),
            ("k2", 0.0, 490.0, 100.0, 100.0),
            ("l", 0.0, 644.0, 300.0, 40.0),
            ("l1", 0.0, 644.0, 240.0, 20.0),
            ("l2", 240.0, 644.0, 60.0, 20.0),
        ],
    },
    Case {
        name: "wrap",
        spec: "§5.2 flex-wrap, §8.3 align-content, §8.1 auto margins, §8.3 baseline",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;flex-wrap:wrap;width:200px;height:150px;background:#eee;margin-bottom:4px}\n.c > div{width:80px;height:30px;background:#8cf}\n</style>\n<div class=c id=a><div id=a1></div><div id=a2></div><div id=a3></div></div>\n<div class=c id=b style=\"align-content:center\"><div id=b1></div><div id=b2></div><div id=b3></div></div>\n<div class=c id=c style=\"align-content:space-between\"><div id=c1></div><div id=c2></div><div id=c3></div></div>\n<div class=c id=d style=\"align-content:flex-end\"><div id=d1></div><div id=d2></div><div id=d3></div></div>\n<div class=c id=e style=\"align-content:space-around\"><div id=e1></div><div id=e2></div><div id=e3></div></div>\n<div class=c id=f style=\"align-content:space-evenly\"><div id=f1></div><div id=f2></div><div id=f3></div></div>\n<div class=c id=g style=\"flex-wrap:nowrap\"><div id=g1></div><div id=g2></div><div id=g3></div></div>\n<div class=c id=h style=\"flex-wrap:wrap;row-gap:12px;column-gap:5px\"><div id=h1></div><div id=h2></div><div id=h3></div></div>\n<div class=c id=i style=\"flex-wrap:nowrap\"><div id=i1 style=\"margin-left:auto\"></div><div id=i2></div></div>\n<div class=c id=j style=\"flex-wrap:nowrap\"><div id=j1 style=\"margin:auto\"></div></div>\n<div class=c id=k style=\"flex-wrap:nowrap\"><div id=k1 style=\"order:2\"></div><div id=k2 style=\"order:1\"></div></div>\n<div class=c id=l style=\"flex-wrap:nowrap;align-items:baseline\"><div id=l1 style=\"height:auto;padding-top:10px\">x</div><div id=l2 style=\"height:auto;padding-top:30px\">y</div></div>\n<div class=c id=m style=\"flex-wrap:nowrap\"><div id=m1 style=\"height:auto;align-self:stretch\"></div></div>\n<div class=c id=n style=\"flex-wrap:wrap;align-content:stretch\"><div id=n1></div><div id=n2></div><div id=n3></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 200.0, 150.0),
            ("a1", 0.0, 0.0, 80.0, 30.0),
            ("a2", 80.0, 0.0, 80.0, 30.0),
            ("a3", 0.0, 75.0, 80.0, 30.0),
            ("b", 0.0, 154.0, 200.0, 150.0),
            ("b1", 0.0, 199.0, 80.0, 30.0),
            ("b2", 80.0, 199.0, 80.0, 30.0),
            ("b3", 0.0, 229.0, 80.0, 30.0),
            ("c", 0.0, 308.0, 200.0, 150.0),
            ("c1", 0.0, 308.0, 80.0, 30.0),
            ("c2", 80.0, 308.0, 80.0, 30.0),
            ("c3", 0.0, 428.0, 80.0, 30.0),
            ("d", 0.0, 462.0, 200.0, 150.0),
            ("d1", 0.0, 552.0, 80.0, 30.0),
            ("d2", 80.0, 552.0, 80.0, 30.0),
            ("d3", 0.0, 582.0, 80.0, 30.0),
            ("e", 0.0, 616.0, 200.0, 150.0),
            ("e1", 0.0, 639.0, 80.0, 30.0),
            ("e2", 80.0, 639.0, 80.0, 30.0),
            ("e3", 0.0, 714.0, 80.0, 30.0),
            ("f", 0.0, 770.0, 200.0, 150.0),
            ("f1", 0.0, 800.0, 80.0, 30.0),
            ("f2", 80.0, 800.0, 80.0, 30.0),
            ("f3", 0.0, 860.0, 80.0, 30.0),
            ("g", 0.0, 924.0, 200.0, 150.0),
            ("g1", 0.0, 924.0, 67.0, 30.0),
            ("g2", 67.0, 924.0, 67.0, 30.0),
            ("g3", 133.0, 924.0, 67.0, 30.0),
            ("h", 0.0, 1078.0, 200.0, 150.0),
            ("h1", 0.0, 1078.0, 80.0, 30.0),
            ("h2", 85.0, 1078.0, 80.0, 30.0),
            ("h3", 0.0, 1159.0, 80.0, 30.0),
            ("i", 0.0, 1232.0, 200.0, 150.0),
            ("i1", 40.0, 1232.0, 80.0, 30.0),
            ("i2", 120.0, 1232.0, 80.0, 30.0),
            ("j", 0.0, 1386.0, 200.0, 150.0),
            ("j1", 60.0, 1446.0, 80.0, 30.0),
            ("k", 0.0, 1540.0, 200.0, 150.0),
            ("k1", 80.0, 1540.0, 80.0, 30.0),
            ("k2", 0.0, 1540.0, 80.0, 30.0),
            ("l", 0.0, 1694.0, 200.0, 150.0),
            ("l1", 0.0, 1714.0, 80.0, 30.0),
            ("l2", 80.0, 1694.0, 80.0, 50.0),
            ("m", 0.0, 1848.0, 200.0, 150.0),
            ("m1", 0.0, 1848.0, 80.0, 150.0),
            ("n", 0.0, 2002.0, 200.0, 150.0),
            ("n1", 0.0, 2002.0, 80.0, 30.0),
            ("n2", 80.0, 2002.0, 80.0, 30.0),
            ("n3", 0.0, 2077.0, 80.0, 30.0),
        ],
    },
    Case {
        name: "misc",
        spec: "§4 box model, §9.2 aspect-ratio, §9.8 percentages, abspos items",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;width:300px;height:60px;background:#eee;margin-bottom:4px}\n.c > div{background:#8cf}\n</style>\n<div class=c id=a><div id=a1 style=\"box-sizing:border-box;flex:1 1 0;padding:10px;border:5px solid #000\"></div><div id=a2 style=\"flex:1 1 0\"></div></div>\n<div class=c id=b><div id=b1 style=\"flex:1 1 0;margin:0 20px\"></div><div id=b2 style=\"flex:1 1 0\"></div></div>\n<div class=c id=c>text<div id=c1 style=\"flex:1\"></div></div>\n<div class=c id=d><div id=d1 style=\"width:50%;height:50%\"></div></div>\n<div class=c id=e style=\"justify-content:center\"><div id=e1 style=\"flex:0 0 200px\"></div><div id=e2 style=\"flex:0 0 200px\"></div></div>\n<div class=c id=f style=\"justify-content:space-between\"><div id=f1 style=\"flex:0 0 200px\"></div><div id=f2 style=\"flex:0 0 200px\"></div></div>\n<div class=c id=g><div id=g1 style=\"flex:1\"><div id=g2 style=\"display:flex;height:20px\"><div id=g3 style=\"flex:1\"></div></div></div></div>\n<div class=c id=h style=\"position:relative\"><div id=h1 style=\"position:absolute;top:5px;left:5px;width:30px;height:10px\"></div><div id=h2 style=\"flex:1\"></div></div>\n<div class=c id=i style=\"flex-flow:column wrap;height:60px\"><div id=i1 style=\"height:40px;width:20px\"></div><div id=i2 style=\"height:40px;width:20px\"></div></div>\n<div class=c id=j><div id=j1 style=\"flex:1;align-self:center;height:20px\"></div><div id=j2 style=\"flex:1;align-self:flex-end;height:20px\"></div></div>\n<div class=c id=k><div id=k1 style=\"flex:0 0 auto;aspect-ratio:2/1;height:30px\"></div></div>\n<div class=c id=l><div id=l1 style=\"flex:2 1 100px\"></div><div id=l2 style=\"flex:1 1 100px\"></div></div>\n<div class=c id=m style=\"flex-direction:column;height:60px\"><div id=m1 style=\"flex:1\"></div><div id=m2 style=\"flex:2\"></div></div>\n<div class=c id=n><div id=n1 style=\"flex:1 1 0;min-width:100px\"></div><div id=n2 style=\"flex:1 1 0;min-width:100px\"></div><div id=n3 style=\"flex:1 1 0\"></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 300.0, 60.0),
            ("a1", 0.0, 0.0, 165.0, 60.0),
            ("a2", 165.0, 0.0, 135.0, 60.0),
            ("b", 0.0, 64.0, 300.0, 60.0),
            ("b1", 20.0, 64.0, 130.0, 60.0),
            ("b2", 170.0, 64.0, 130.0, 60.0),
            ("c", 0.0, 128.0, 300.0, 60.0),
            ("c1", 39.0, 128.0, 261.0, 60.0),
            ("d", 0.0, 192.0, 300.0, 60.0),
            ("d1", 0.0, 192.0, 150.0, 30.0),
            ("e", 0.0, 256.0, 300.0, 60.0),
            ("e1", -50.0, 256.0, 200.0, 60.0),
            ("e2", 150.0, 256.0, 200.0, 60.0),
            ("f", 0.0, 320.0, 300.0, 60.0),
            ("f1", 0.0, 320.0, 200.0, 60.0),
            ("f2", 200.0, 320.0, 200.0, 60.0),
            ("g", 0.0, 384.0, 300.0, 60.0),
            ("g1", 0.0, 384.0, 300.0, 60.0),
            ("g2", 0.0, 384.0, 300.0, 20.0),
            ("g3", 0.0, 384.0, 300.0, 20.0),
            ("h", 0.0, 448.0, 300.0, 60.0),
            ("h1", 5.0, 453.0, 30.0, 10.0),
            ("h2", 0.0, 448.0, 300.0, 60.0),
            ("i", 0.0, 512.0, 300.0, 60.0),
            ("i1", 0.0, 512.0, 20.0, 40.0),
            ("i2", 150.0, 512.0, 20.0, 40.0),
            ("j", 0.0, 576.0, 300.0, 60.0),
            ("j1", 0.0, 596.0, 150.0, 20.0),
            ("j2", 150.0, 616.0, 150.0, 20.0),
            ("k", 0.0, 640.0, 300.0, 60.0),
            ("k1", 0.0, 640.0, 60.0, 30.0),
            ("l", 0.0, 704.0, 300.0, 60.0),
            ("l1", 0.0, 704.0, 167.0, 60.0),
            ("l2", 167.0, 704.0, 133.0, 60.0),
            ("m", 0.0, 768.0, 300.0, 60.0),
            ("m1", 0.0, 768.0, 300.0, 20.0),
            ("m2", 0.0, 788.0, 300.0, 40.0),
            ("n", 0.0, 832.0, 300.0, 60.0),
            ("n1", 0.0, 832.0, 100.0, 60.0),
            ("n2", 100.0, 832.0, 100.0, 60.0),
            ("n3", 200.0, 832.0, 100.0, 60.0),
        ],
    },
    Case {
        name: "overflow",
        spec: "Box Alignment §4.4 overflow fallbacks",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;flex-wrap:wrap;width:100px;height:40px;background:#eee;margin-bottom:4px}\n.c > div{width:80px;height:30px;background:#8cf}\n.r{display:flex;width:300px;height:30px;background:#eee;margin-bottom:4px}\n.r > div{flex:0 0 200px;height:20px;background:#fc8}\n</style>\n<div class=c id=a style=\"align-content:flex-start\"><div id=a1></div><div id=a2></div><div id=a3></div></div>\n<div class=c id=b style=\"align-content:center\"><div id=b1></div><div id=b2></div><div id=b3></div></div>\n<div class=c id=c style=\"align-content:flex-end\"><div id=c1></div><div id=c2></div><div id=c3></div></div>\n<div class=c id=d style=\"align-content:space-between\"><div id=d1></div><div id=d2></div><div id=d3></div></div>\n<div class=c id=e style=\"align-content:space-around\"><div id=e1></div><div id=e2></div><div id=e3></div></div>\n<div class=c id=f style=\"align-content:space-evenly\"><div id=f1></div><div id=f2></div><div id=f3></div></div>\n<div class=c id=g style=\"align-content:stretch\"><div id=g1></div><div id=g2></div><div id=g3></div></div>\n<div class=r id=h style=\"justify-content:space-around\"><div id=h1></div><div id=h2></div></div>\n<div class=r id=i style=\"justify-content:space-evenly\"><div id=i1></div><div id=i2></div></div>\n<div class=r id=j style=\"justify-content:flex-end\"><div id=j1></div><div id=j2></div></div>\n<div class=r id=k style=\"justify-content:space-between;flex-direction:row-reverse\"><div id=k1></div><div id=k2></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 100.0, 40.0),
            ("a1", 0.0, 0.0, 80.0, 30.0),
            ("a2", 0.0, 30.0, 80.0, 30.0),
            ("a3", 0.0, 60.0, 80.0, 30.0),
            ("b", 0.0, 44.0, 100.0, 40.0),
            ("b1", 0.0, 19.0, 80.0, 30.0),
            ("b2", 0.0, 49.0, 80.0, 30.0),
            ("b3", 0.0, 79.0, 80.0, 30.0),
            ("c", 0.0, 88.0, 100.0, 40.0),
            ("c1", 0.0, 38.0, 80.0, 30.0),
            ("c2", 0.0, 68.0, 80.0, 30.0),
            ("c3", 0.0, 98.0, 80.0, 30.0),
            ("d", 0.0, 132.0, 100.0, 40.0),
            ("d1", 0.0, 132.0, 80.0, 30.0),
            ("d2", 0.0, 162.0, 80.0, 30.0),
            ("d3", 0.0, 192.0, 80.0, 30.0),
            ("e", 0.0, 176.0, 100.0, 40.0),
            ("e1", 0.0, 176.0, 80.0, 30.0),
            ("e2", 0.0, 206.0, 80.0, 30.0),
            ("e3", 0.0, 236.0, 80.0, 30.0),
            ("f", 0.0, 220.0, 100.0, 40.0),
            ("f1", 0.0, 220.0, 80.0, 30.0),
            ("f2", 0.0, 250.0, 80.0, 30.0),
            ("f3", 0.0, 280.0, 80.0, 30.0),
            ("g", 0.0, 264.0, 100.0, 40.0),
            ("g1", 0.0, 264.0, 80.0, 30.0),
            ("g2", 0.0, 294.0, 80.0, 30.0),
            ("g3", 0.0, 324.0, 80.0, 30.0),
            ("h", 0.0, 308.0, 300.0, 30.0),
            ("h1", 0.0, 308.0, 200.0, 20.0),
            ("h2", 200.0, 308.0, 200.0, 20.0),
            ("i", 0.0, 342.0, 300.0, 30.0),
            ("i1", 0.0, 342.0, 200.0, 20.0),
            ("i2", 200.0, 342.0, 200.0, 20.0),
            ("j", 0.0, 376.0, 300.0, 30.0),
            ("j1", -100.0, 376.0, 200.0, 20.0),
            ("j2", 100.0, 376.0, 200.0, 20.0),
            ("k", 0.0, 410.0, 300.0, 30.0),
            ("k1", 100.0, 410.0, 200.0, 20.0),
            ("k2", -100.0, 410.0, 200.0, 20.0),
        ],
    },
    Case {
        name: "more",
        spec: "§7.1 the flex shorthand, §4.5 automatic minimum",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;width:300px;height:60px;background:#eee;margin-bottom:4px}\n.c > div{background:#8cf}\n</style>\n<div class=c id=a><div id=a1 style=\"flex:2\"></div><div id=a2 style=\"flex:1\"></div></div>\n<div class=c id=b><div id=b1 style=\"flex:0 0 30%\"></div><div id=b2 style=\"flex:1 0 auto\"></div></div>\n<div class=c id=c><div id=c1 style=\"flex:1 1 100%\"></div><div id=c2 style=\"flex:1 1 100%\"></div></div>\n<div class=c id=d><div id=d1 style=\"flex-basis:content;width:10px\">wide text here</div><div id=d2 style=\"flex:1\"></div></div>\n<div class=c id=e><div id=e1 style=\"flex:1 1 auto;min-width:auto\">aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</div><div id=e2 style=\"flex:1 1 auto\"></div></div>\n<div class=c id=f><div id=f1 style=\"flex:1 1 auto;overflow:hidden\">aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</div><div id=f2 style=\"flex:0 0 250px\"></div></div>\n<div class=c id=g style=\"flex-direction:column;align-items:flex-start;height:90px\"><div id=g1 style=\"flex:1\">a</div><div id=g2 style=\"flex:1\">bbbb</div></div>\n<div class=c id=h><div id=h1 style=\"flex:1;visibility:hidden\"></div><div id=h2 style=\"flex:1\"></div></div>\n<div class=c id=i style=\"padding:10px;border:5px solid #000;box-sizing:border-box\"><div id=i1 style=\"flex:1\"></div><div id=i2 style=\"flex:1\"></div></div>\n<div class=c id=j style=\"flex-wrap:wrap;width:250px\"><div id=j1 style=\"flex:1 1 100px\"></div><div id=j2 style=\"flex:1 1 100px\"></div><div id=j3 style=\"flex:1 1 100px\"></div></div>\n<div class=c id=k><div id=k1 style=\"flex:1;max-width:none\"></div><div id=k2 style=\"flex:1;min-width:0\"></div></div>\n<div class=c id=l style=\"flex-direction:column\"><div id=l1 style=\"flex:0 0 auto;max-height:20px;height:50px\"></div></div>\n<div class=c id=m><div id=m1 style=\"flex:1 1 0;margin-right:auto;width:40px\"></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 300.0, 60.0),
            ("a1", 0.0, 0.0, 200.0, 60.0),
            ("a2", 200.0, 0.0, 100.0, 60.0),
            ("b", 0.0, 64.0, 300.0, 60.0),
            ("b1", 0.0, 64.0, 90.0, 60.0),
            ("b2", 90.0, 64.0, 210.0, 60.0),
            ("c", 0.0, 128.0, 300.0, 60.0),
            ("c1", 0.0, 128.0, 150.0, 60.0),
            ("c2", 150.0, 128.0, 150.0, 60.0),
            ("d", 0.0, 192.0, 300.0, 60.0),
            ("d1", 0.0, 192.0, 135.0, 60.0),
            ("d2", 135.0, 192.0, 165.0, 60.0),
            ("e", 0.0, 256.0, 300.0, 60.0),
            ("e1", 0.0, 256.0, 347.0, 60.0),
            ("e2", 347.0, 256.0, 0.0, 60.0),
            ("f", 0.0, 320.0, 300.0, 60.0),
            ("f1", 0.0, 320.0, 50.0, 60.0),
            ("f2", 50.0, 320.0, 250.0, 60.0),
            ("g", 0.0, 384.0, 300.0, 90.0),
            ("g1", 0.0, 384.0, 10.0, 45.0),
            ("g2", 0.0, 429.0, 39.0, 45.0),
            ("h", 0.0, 478.0, 300.0, 60.0),
            ("h1", 0.0, 478.0, 150.0, 60.0),
            ("h2", 150.0, 478.0, 150.0, 60.0),
            ("i", 0.0, 542.0, 300.0, 60.0),
            ("i1", 15.0, 557.0, 135.0, 30.0),
            ("i2", 150.0, 557.0, 135.0, 30.0),
            ("j", 0.0, 606.0, 250.0, 60.0),
            ("j1", 0.0, 606.0, 125.0, 30.0),
            ("j2", 125.0, 606.0, 125.0, 30.0),
            ("j3", 0.0, 636.0, 250.0, 30.0),
            ("k", 0.0, 670.0, 300.0, 60.0),
            ("k1", 0.0, 670.0, 150.0, 60.0),
            ("k2", 150.0, 670.0, 150.0, 60.0),
            ("l", 0.0, 734.0, 300.0, 60.0),
            ("l1", 0.0, 734.0, 300.0, 20.0),
            ("m", 0.0, 798.0, 300.0, 60.0),
            ("m1", 0.0, 798.0, 300.0, 60.0),
        ],
    },
    Case {
        name: "edge",
        spec: "§8.3 gaps, wrap-reverse, column-reverse, nested",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;width:300px;height:60px;background:#eee;margin-bottom:4px}\n.c > div{background:#8cf}\n</style>\n<div class=c id=a style=\"gap:10%\"><div id=a1 style=\"flex:1\"></div><div id=a2 style=\"flex:1\"></div></div>\n<div class=c id=b style=\"flex-direction:column;height:120px;row-gap:10%\"><div id=b1 style=\"flex:1\"></div><div id=b2 style=\"flex:1\"></div></div>\n<div class=c id=c style=\"flex-direction:column-reverse;height:90px\"><div id=c1 style=\"height:30px\"></div><div id=c2 style=\"height:30px\"></div></div>\n<div class=c id=d style=\"flex-wrap:wrap-reverse;width:150px\"><div id=d1 style=\"width:100px;height:20px\"></div><div id=d2 style=\"width:100px;height:20px\"></div></div>\n<div class=c id=e><div id=e1 style=\"flex:1;margin-left:-20px\"></div><div id=e2 style=\"flex:1\"></div></div>\n<div class=c id=f style=\"flex-direction:column;height:auto\"><div id=f1>one</div><div id=f2>two</div></div>\n<div class=c id=g><div id=g1 style=\"display:flex;flex:1\"><div id=g2 style=\"flex:1;height:10px\"></div></div></div>\n<div class=c id=h><div id=h1 style=\"flex:0 0 auto\"><div style=\"width:120px;height:10px\"></div></div><div id=h2 style=\"flex:1\"></div></div>\n<div class=c id=i style=\"align-items:flex-end\"><div id=i1 style=\"height:20px;align-self:auto\"></div><div id=i2 style=\"height:20px;align-self:center\"></div></div>\n<div class=c id=j style=\"flex-direction:column;height:40px\"><div id=j1 style=\"flex:0 0 auto\">aaa bbb ccc ddd eee fff ggg hhh</div></div>\n<div class=c id=k style=\"width:0\"><div id=k1 style=\"flex:1 1 auto\">aaaa</div></div>\n<div class=c id=l><div id=l1 style=\"flex:1 1 0;height:0;min-height:15px\"></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 300.0, 60.0),
            ("a1", 0.0, 0.0, 135.0, 60.0),
            ("a2", 165.0, 0.0, 135.0, 60.0),
            ("b", 0.0, 64.0, 300.0, 120.0),
            ("b1", 0.0, 64.0, 300.0, 54.0),
            ("b2", 0.0, 130.0, 300.0, 54.0),
            ("c", 0.0, 188.0, 300.0, 90.0),
            ("c1", 0.0, 248.0, 300.0, 30.0),
            ("c2", 0.0, 218.0, 300.0, 30.0),
            ("d", 0.0, 282.0, 150.0, 60.0),
            ("d1", 0.0, 322.0, 100.0, 20.0),
            ("d2", 0.0, 292.0, 100.0, 20.0),
            ("e", 0.0, 346.0, 300.0, 60.0),
            ("e1", -20.0, 346.0, 160.0, 60.0),
            ("e2", 140.0, 346.0, 160.0, 60.0),
            ("f", 0.0, 410.0, 300.0, 40.0),
            ("f1", 0.0, 410.0, 300.0, 20.0),
            ("f2", 0.0, 430.0, 300.0, 20.0),
            ("g", 0.0, 454.0, 300.0, 60.0),
            ("g1", 0.0, 454.0, 300.0, 60.0),
            ("g2", 0.0, 454.0, 300.0, 10.0),
            ("h", 0.0, 518.0, 300.0, 60.0),
            ("h1", 0.0, 518.0, 120.0, 60.0),
            ("h2", 120.0, 518.0, 180.0, 60.0),
            ("i", 0.0, 582.0, 300.0, 60.0),
            ("i1", 0.0, 622.0, 0.0, 20.0),
            ("i2", 0.0, 602.0, 0.0, 20.0),
            ("j", 0.0, 646.0, 300.0, 40.0),
            ("j1", 0.0, 646.0, 300.0, 20.0),
            ("k", 0.0, 690.0, 0.0, 60.0),
            ("k1", 0.0, 690.0, 39.0, 60.0),
            ("l", 0.0, 754.0, 300.0, 60.0),
            ("l1", 0.0, 754.0, 300.0, 15.0),
        ],
    },
    Case {
        name: "kw",
        spec: "Cascade §7 CSS-wide keywords on shorthands",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;width:300px;height:30px;background:#eee;margin-bottom:4px}\n.c > div{height:20px;background:#8cf}\n.pre{flex:1 1 200px}\n</style>\n<div class=c><div id=a1 class=pre style=\"flex:initial\"></div><div id=a2 style=\"flex:0 0 100px\"></div></div>\n<div class=c><div id=b1 class=pre style=\"flex:unset\"></div><div id=b2 style=\"flex:0 0 100px\"></div></div>\n<div class=c><div id=c1 class=pre style=\"flex:revert\"></div><div id=c2 style=\"flex:0 0 100px\"></div></div>\n<div class=c><div id=d1 class=pre style=\"flex-grow:initial\"></div><div id=d2 style=\"flex:0 0 100px\"></div></div>\n<div class=c style=\"margin-bottom:initial\"><div id=e1 style=\"flex:1\"></div></div>\n<div class=c><div id=f1 style=\"margin:20px;margin:initial;flex:1\"></div></div>\n<div class=c><div id=g1 style=\"padding:10px;padding:unset;flex:1\"></div></div>\n<div class=c><div id=h1 style=\"flex:1 1 200px;flex:initial\"></div><div id=h2 style=\"flex:0 0 100px\"></div></div>",
        expect: &[
            ("a1", 0.0, 0.0, 0.0, 20.0),
            ("a2", 0.0, 0.0, 100.0, 20.0),
            ("b1", 0.0, 34.0, 0.0, 20.0),
            ("b2", 0.0, 34.0, 100.0, 20.0),
            ("c1", 0.0, 68.0, 0.0, 20.0),
            ("c2", 0.0, 68.0, 100.0, 20.0),
            ("d1", 0.0, 102.0, 200.0, 20.0),
            ("d2", 200.0, 102.0, 100.0, 20.0),
            ("e1", 0.0, 136.0, 300.0, 20.0),
            ("f1", 0.0, 166.0, 300.0, 20.0),
            ("g1", 0.0, 200.0, 300.0, 20.0),
            ("h1", 0.0, 234.0, 0.0, 20.0),
            ("h2", 0.0, 234.0, 100.0, 20.0),
        ],
    },
    Case {
        name: "vals",
        spec: "Box Alignment §4-5 the full value grammar",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;width:300px;height:60px;background:#eee;margin-bottom:4px}\n.c > div{background:#8cf;width:80px;height:20px}\n.o > div{width:200px}\n</style>\n<div class=c id=a style=\"justify-content:left\"><div id=a1></div></div>\n<div class=c id=b style=\"justify-content:right\"><div id=b1></div></div>\n<div class=c id=c style=\"justify-content:start\"><div id=c1></div></div>\n<div class=c id=d style=\"justify-content:end\"><div id=d1></div></div>\n<div class=c id=e style=\"justify-content:normal\"><div id=e1></div></div>\n<div class=c id=f style=\"justify-content:safe center\"><div id=f1></div></div>\n<div class=c id=g style=\"justify-content:unsafe center\"><div id=g1></div></div>\n<div class=\"c o\" id=h style=\"justify-content:safe center\"><div id=h1></div><div id=h2></div></div>\n<div class=\"c o\" id=i style=\"justify-content:unsafe center\"><div id=i1></div><div id=i2></div></div>\n<div class=c id=j style=\"align-items:normal\"><div id=j1></div></div>\n<div class=c id=k style=\"align-items:start\"><div id=k1></div></div>\n<div class=c id=l style=\"align-items:end\"><div id=l1></div></div>\n<div class=c id=m style=\"align-items:safe center\"><div id=m1></div></div>\n<div class=c id=n style=\"align-items:first baseline\"><div id=n1 style=\"height:auto;padding-top:10px\">x</div><div id=n2 style=\"height:auto;padding-top:30px\">y</div></div>\n<div class=c id=o style=\"align-items:last baseline\"><div id=o1 style=\"height:auto;padding-top:10px\">x</div><div id=o2 style=\"height:auto;padding-top:30px\">y</div></div>\n<div class=c id=p style=\"align-content:end;flex-wrap:wrap\"><div id=p1></div></div>\n<div class=c id=q style=\"gap:10px 30px;flex-wrap:wrap;width:200px\"><div id=q1></div><div id=q2></div><div id=q3></div></div>\n<div class=c id=r style=\"place-content:end center;flex-wrap:wrap\"><div id=r1></div></div>\n<div class=c id=s style=\"place-items:end\"><div id=s1></div></div>\n<div class=c id=t><div id=t1 style=\"place-self:end\"></div></div>\n<div class=c id=u style=\"flex-flow:wrap column;height:60px\"><div id=u1 style=\"height:40px\"></div><div id=u2 style=\"height:40px\"></div></div>\n<div class=c id=v><div id=v1 style=\"align-self:normal\"></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 300.0, 60.0),
            ("a1", 0.0, 0.0, 80.0, 20.0),
            ("b", 0.0, 64.0, 300.0, 60.0),
            ("b1", 220.0, 64.0, 80.0, 20.0),
            ("c", 0.0, 128.0, 300.0, 60.0),
            ("c1", 0.0, 128.0, 80.0, 20.0),
            ("d", 0.0, 192.0, 300.0, 60.0),
            ("d1", 220.0, 192.0, 80.0, 20.0),
            ("e", 0.0, 256.0, 300.0, 60.0),
            ("e1", 0.0, 256.0, 80.0, 20.0),
            ("f", 0.0, 320.0, 300.0, 60.0),
            ("f1", 110.0, 320.0, 80.0, 20.0),
            ("g", 0.0, 384.0, 300.0, 60.0),
            ("g1", 110.0, 384.0, 80.0, 20.0),
            ("h", 0.0, 448.0, 300.0, 60.0),
            ("h1", 0.0, 448.0, 150.0, 20.0),
            ("h2", 150.0, 448.0, 150.0, 20.0),
            ("i", 0.0, 512.0, 300.0, 60.0),
            ("i1", 0.0, 512.0, 150.0, 20.0),
            ("i2", 150.0, 512.0, 150.0, 20.0),
            ("j", 0.0, 576.0, 300.0, 60.0),
            ("j1", 0.0, 576.0, 80.0, 20.0),
            ("k", 0.0, 640.0, 300.0, 60.0),
            ("k1", 0.0, 640.0, 80.0, 20.0),
            ("l", 0.0, 704.0, 300.0, 60.0),
            ("l1", 0.0, 744.0, 80.0, 20.0),
            ("m", 0.0, 768.0, 300.0, 60.0),
            ("m1", 0.0, 788.0, 80.0, 20.0),
            ("n", 0.0, 832.0, 300.0, 60.0),
            ("n1", 0.0, 852.0, 80.0, 30.0),
            ("n2", 80.0, 832.0, 80.0, 50.0),
            ("o", 0.0, 896.0, 300.0, 60.0),
            ("o1", 0.0, 926.0, 80.0, 30.0),
            ("o2", 80.0, 906.0, 80.0, 50.0),
            ("p", 0.0, 960.0, 300.0, 60.0),
            ("p1", 0.0, 1000.0, 80.0, 20.0),
            ("q", 0.0, 1024.0, 200.0, 60.0),
            ("q1", 0.0, 1024.0, 80.0, 20.0),
            ("q2", 110.0, 1024.0, 80.0, 20.0),
            ("q3", 0.0, 1059.0, 80.0, 20.0),
            ("r", 0.0, 1088.0, 300.0, 60.0),
            ("r1", 110.0, 1128.0, 80.0, 20.0),
            ("s", 0.0, 1152.0, 300.0, 60.0),
            ("s1", 0.0, 1192.0, 80.0, 20.0),
            ("t", 0.0, 1216.0, 300.0, 60.0),
            ("t1", 0.0, 1256.0, 80.0, 20.0),
            ("u", 0.0, 1280.0, 300.0, 60.0),
            ("u1", 0.0, 1280.0, 80.0, 40.0),
            ("u2", 150.0, 1280.0, 80.0, 40.0),
            ("v", 0.0, 1344.0, 300.0, 60.0),
            ("v1", 0.0, 1344.0, 80.0, 20.0),
        ],
    },
    Case {
        name: "safe",
        spec: "Box Alignment §4.4 safe / unsafe",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;width:300px;height:30px;background:#eee;margin-bottom:4px}\n.c > div{background:#8cf;height:20px;flex:0 0 200px}\n.t{display:flex;flex-wrap:wrap;width:100px;height:40px;background:#eee;margin-bottom:4px}\n.t > div{width:80px;height:30px;background:#fc8}\n</style>\n<div class=c id=a style=\"justify-content:safe center\"><div id=a1></div><div id=a2></div></div>\n<div class=c id=b style=\"justify-content:unsafe center\"><div id=b1></div><div id=b2></div></div>\n<div class=c id=c style=\"justify-content:safe flex-end\"><div id=c1></div><div id=c2></div></div>\n<div class=c id=d style=\"justify-content:unsafe flex-end\"><div id=d1></div><div id=d2></div></div>\n<div class=t id=e style=\"align-content:safe center\"><div id=e1></div><div id=e2></div><div id=e3></div></div>\n<div class=t id=f style=\"align-content:unsafe center\"><div id=f1></div><div id=f2></div><div id=f3></div></div>\n<div class=c id=g style=\"align-items:safe center\"><div id=g1 style=\"height:50px\"></div></div>\n<div class=c id=h style=\"align-items:unsafe center\"><div id=h1 style=\"height:50px\"></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 300.0, 30.0),
            ("a1", 0.0, 0.0, 200.0, 20.0),
            ("a2", 200.0, 0.0, 200.0, 20.0),
            ("b", 0.0, 34.0, 300.0, 30.0),
            ("b1", -50.0, 34.0, 200.0, 20.0),
            ("b2", 150.0, 34.0, 200.0, 20.0),
            ("c", 0.0, 68.0, 300.0, 30.0),
            ("c1", 0.0, 68.0, 200.0, 20.0),
            ("c2", 200.0, 68.0, 200.0, 20.0),
            ("d", 0.0, 102.0, 300.0, 30.0),
            ("d1", -100.0, 102.0, 200.0, 20.0),
            ("d2", 100.0, 102.0, 200.0, 20.0),
            ("e", 0.0, 136.0, 100.0, 40.0),
            ("e1", 0.0, 136.0, 80.0, 30.0),
            ("e2", 0.0, 166.0, 80.0, 30.0),
            ("e3", 0.0, 196.0, 80.0, 30.0),
            ("f", 0.0, 180.0, 100.0, 40.0),
            ("f1", 0.0, 155.0, 80.0, 30.0),
            ("f2", 0.0, 185.0, 80.0, 30.0),
            ("f3", 0.0, 215.0, 80.0, 30.0),
            ("g", 0.0, 224.0, 300.0, 30.0),
            ("g1", 0.0, 224.0, 200.0, 50.0),
            ("h", 0.0, 258.0, 300.0, 30.0),
            ("h1", 0.0, 248.0, 200.0, 50.0),
        ],
    },
    Case {
        name: "edge2",
        spec: "§9.4 line cross sizing, wrap-reverse + safe",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.t{display:flex;flex-wrap:wrap;width:200px;height:100px;background:#eee;margin-bottom:4px}\n.t > div{width:80px;height:30px;background:#8cf}\n</style>\n<div class=t id=a style=\"align-content:center\"><div id=a1></div></div>\n<div class=t id=b style=\"align-content:flex-end\"><div id=b1></div></div>\n<div class=t id=c style=\"align-content:stretch\"><div id=c1></div></div>\n<div class=t id=d style=\"flex-wrap:wrap-reverse;height:20px;align-items:safe center\"><div id=d1 style=\"height:50px\"></div></div>\n<div class=t id=e style=\"flex-wrap:wrap-reverse;height:20px;align-items:unsafe center\"><div id=e1 style=\"height:50px\"></div></div>\n<div class=t id=f style=\"flex-wrap:wrap-reverse;height:80px;align-items:flex-start\"><div id=f1 style=\"height:30px\"></div></div>\n<div class=t id=g style=\"flex-wrap:nowrap;height:100px;align-content:center\"><div id=g1></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 200.0, 100.0),
            ("a1", 0.0, 35.0, 80.0, 30.0),
            ("b", 0.0, 104.0, 200.0, 100.0),
            ("b1", 0.0, 174.0, 80.0, 30.0),
            ("c", 0.0, 208.0, 200.0, 100.0),
            ("c1", 0.0, 208.0, 80.0, 30.0),
            ("d", 0.0, 312.0, 200.0, 20.0),
            ("d1", 0.0, 282.0, 80.0, 50.0),
            ("e", 0.0, 336.0, 200.0, 20.0),
            ("e1", 0.0, 306.0, 80.0, 50.0),
            ("f", 0.0, 360.0, 200.0, 80.0),
            ("f1", 0.0, 410.0, 80.0, 30.0),
            ("g", 0.0, 444.0, 200.0, 100.0),
            ("g1", 0.0, 444.0, 80.0, 30.0),
        ],
    },
    Case {
        name: "wr",
        spec: "§5.2 wrap-reverse flips align-content",
        html: "<style>body{margin:0;font:16px/20px monospace}\n.t{display:flex;flex-wrap:wrap-reverse;width:200px;height:100px;background:#eee;margin-bottom:4px}\n.t > div{width:80px;height:30px;background:#8cf}</style>\n<div class=t id=a style=\"align-content:flex-start\"><div id=a1></div></div>\n<div class=t id=b style=\"align-content:flex-end\"><div id=b1></div></div>\n<div class=t id=c style=\"align-content:center\"><div id=c1></div></div>\n<div class=t id=d style=\"align-content:stretch\"><div id=d1></div></div>\n<div class=t id=e style=\"height:40px;align-content:flex-start\"><div id=e1></div><div id=e2></div><div id=e3></div></div>\n<div class=t id=f style=\"height:40px;align-content:flex-end\"><div id=f1></div><div id=f2></div><div id=f3></div></div>\n<div class=t id=g style=\"height:20px\"><div id=g1 style=\"height:50px\"></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 200.0, 100.0),
            ("a1", 0.0, 70.0, 80.0, 30.0),
            ("b", 0.0, 104.0, 200.0, 100.0),
            ("b1", 0.0, 104.0, 80.0, 30.0),
            ("c", 0.0, 208.0, 200.0, 100.0),
            ("c1", 0.0, 243.0, 80.0, 30.0),
            ("d", 0.0, 312.0, 200.0, 100.0),
            ("d1", 0.0, 382.0, 80.0, 30.0),
            ("e", 0.0, 416.0, 200.0, 40.0),
            ("e1", 0.0, 426.0, 80.0, 30.0),
            ("e2", 80.0, 426.0, 80.0, 30.0),
            ("e3", 0.0, 396.0, 80.0, 30.0),
            ("f", 0.0, 460.0, 200.0, 40.0),
            ("f1", 0.0, 490.0, 80.0, 30.0),
            ("f2", 80.0, 490.0, 80.0, 30.0),
            ("f3", 0.0, 460.0, 80.0, 30.0),
            ("g", 0.0, 504.0, 200.0, 20.0),
            ("g1", 0.0, 474.0, 80.0, 50.0),
        ],
    },
    Case {
        name: "intr",
        spec: "Sizing §5 intrinsic keywords, abspos align-self",
        html: "<style>body{margin:0;font:16px/20px monospace}\n.c{display:flex;width:300px;background:#eee;margin-bottom:4px}\n.c > div{background:#8cf}</style>\n<div class=c id=a><div id=a1 style=\"flex-basis:min-content\">aa bbbb cc</div><div id=a2 style=\"flex:0 0 100px;height:20px\"></div></div>\n<div class=c id=b><div id=b1 style=\"flex-basis:max-content\">aa bbbb cc</div><div id=b2 style=\"flex:0 0 100px;height:20px\"></div></div>\n<div class=c id=c><div id=c1 style=\"flex-basis:fit-content\">aa bbbb cc</div><div id=c2 style=\"flex:0 0 100px;height:20px\"></div></div>\n<div class=c id=d><div id=d1 style=\"width:min-content\">aa bbbb cc</div></div>\n<div class=c id=e style=\"flex-direction:column;height:40px\"><div id=e1 style=\"flex:0 1 auto\">aa bb cc dd ee ff gg hh ii jj kk ll</div></div>\n<div class=c id=f style=\"flex-direction:column;height:40px\"><div id=f1 style=\"flex:0 1 auto;overflow:hidden\">aa bb cc dd ee ff gg hh ii jj kk ll</div></div>\n<div class=c id=g style=\"position:relative;height:60px\"><div id=g1 style=\"position:absolute;align-self:center;height:20px;width:30px\"></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 300.0, 60.0),
            ("a1", 0.0, 0.0, 39.0, 60.0),
            ("a2", 39.0, 0.0, 100.0, 20.0),
            ("b", 0.0, 64.0, 300.0, 20.0),
            ("b1", 0.0, 64.0, 96.0, 20.0),
            ("b2", 96.0, 64.0, 100.0, 20.0),
            ("c", 0.0, 88.0, 300.0, 20.0),
            ("c1", 0.0, 88.0, 96.0, 20.0),
            ("c2", 96.0, 88.0, 100.0, 20.0),
            ("d", 0.0, 112.0, 300.0, 60.0),
            ("d1", 0.0, 112.0, 39.0, 60.0),
            ("e", 0.0, 176.0, 300.0, 40.0),
            ("e1", 0.0, 176.0, 300.0, 40.0),
            ("f", 0.0, 220.0, 300.0, 40.0),
            ("f1", 0.0, 220.0, 300.0, 40.0),
            ("g", 0.0, 264.0, 300.0, 60.0),
            ("g1", 0.0, 284.0, 30.0, 20.0),
        ],
    },
    Case {
        name: "colwrap",
        spec: "§9.2 collect into lines with an indefinite main size",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;background:#eee;margin-bottom:4px}\n.c > div{background:#8cf;width:50px;height:30px}\n</style>\n<div class=c id=a style=\"flex-direction:column;flex-wrap:wrap;width:200px\"><div id=a1></div><div id=a2></div><div id=a3></div></div>\n<div class=c id=b style=\"flex-direction:column;flex-wrap:wrap;width:200px;height:70px\"><div id=b1></div><div id=b2></div><div id=b3></div></div>\n<div class=c id=c style=\"flex-direction:row;flex-wrap:wrap;width:200px\"><div id=c1 style=\"width:80px\"></div><div id=c2 style=\"width:80px\"></div><div id=c3 style=\"width:80px\"></div></div>\n<div class=c id=d style=\"flex-direction:column;flex-wrap:wrap;width:200px;min-height:70px\"><div id=d1></div><div id=d2></div><div id=d3></div></div>\n<div class=c id=e style=\"flex-direction:column-reverse;flex-wrap:wrap;width:200px\"><div id=e1></div><div id=e2></div><div id=e3></div></div>\n<div class=c id=f style=\"flex-direction:column;flex-wrap:wrap;width:200px;align-content:center\"><div id=f1></div><div id=f2></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 200.0, 90.0),
            ("a1", 0.0, 0.0, 50.0, 30.0),
            ("a2", 0.0, 30.0, 50.0, 30.0),
            ("a3", 0.0, 60.0, 50.0, 30.0),
            ("b", 0.0, 94.0, 200.0, 70.0),
            ("b1", 0.0, 94.0, 50.0, 30.0),
            ("b2", 0.0, 124.0, 50.0, 30.0),
            ("b3", 100.0, 94.0, 50.0, 30.0),
            ("c", 0.0, 168.0, 200.0, 60.0),
            ("c1", 0.0, 168.0, 80.0, 30.0),
            ("c2", 80.0, 168.0, 80.0, 30.0),
            ("c3", 0.0, 198.0, 80.0, 30.0),
            ("d", 0.0, 232.0, 200.0, 90.0),
            ("d1", 0.0, 232.0, 50.0, 30.0),
            ("d2", 0.0, 262.0, 50.0, 30.0),
            ("d3", 0.0, 292.0, 50.0, 30.0),
            ("e", 0.0, 326.0, 200.0, 90.0),
            ("e1", 0.0, 386.0, 50.0, 30.0),
            ("e2", 0.0, 356.0, 50.0, 30.0),
            ("e3", 0.0, 326.0, 50.0, 30.0),
            ("f", 0.0, 420.0, 200.0, 60.0),
            ("f1", 75.0, 420.0, 50.0, 30.0),
            ("f2", 75.0, 450.0, 50.0, 30.0),
        ],
    },
    Case {
        name: "frac",
        spec: "§9.7 flex factors that sum to less than one",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;width:300px;height:40px;background:#eee;margin-bottom:4px}\n.c > div{height:20px;background:#8cf}\n</style>\n<div class=c id=a><div id=a1 style=\"flex:0.5 0 0px\"></div><div id=a2 style=\"flex:0 0 100px\"></div></div>\n<div class=c id=b><div id=b1 style=\"flex:0.25 0 0px\"></div><div id=b2 style=\"flex:0.25 0 0px\"></div></div>\n<div class=c id=c><div id=c1 style=\"flex:0 0.5 400px;min-width:0\"></div></div>\n<div class=c id=d><div id=d1 style=\"flex:0 0.25 400px;min-width:0\"></div><div id=d2 style=\"flex:0 0.25 200px;min-width:0\"></div></div>\n<div class=c id=e><div id=e1 style=\"flex:0.3 0 0px\"></div><div id=e2 style=\"flex:0.4 0 0px\"></div></div>\n<div class=c id=f><div id=f1 style=\"flex:2 0 0px\"></div><div id=f2 style=\"flex:0.5 0 0px\"></div></div>\n<div class=c id=g><div id=g1 style=\"flex:0.5 0 0px;max-width:60px\"></div><div id=g2 style=\"flex:0.5 0 0px\"></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 300.0, 40.0),
            ("a1", 0.0, 0.0, 100.0, 20.0),
            ("a2", 100.0, 0.0, 100.0, 20.0),
            ("b", 0.0, 44.0, 300.0, 40.0),
            ("b1", 0.0, 44.0, 75.0, 20.0),
            ("b2", 75.0, 44.0, 75.0, 20.0),
            ("c", 0.0, 88.0, 300.0, 40.0),
            ("c1", 0.0, 88.0, 350.0, 20.0),
            ("d", 0.0, 132.0, 300.0, 40.0),
            ("d1", 0.0, 132.0, 300.0, 20.0),
            ("d2", 300.0, 132.0, 150.0, 20.0),
            ("e", 0.0, 176.0, 300.0, 40.0),
            ("e1", 0.0, 176.0, 90.0, 20.0),
            ("e2", 90.0, 176.0, 120.0, 20.0),
            ("f", 0.0, 220.0, 300.0, 40.0),
            ("f1", 0.0, 220.0, 240.0, 20.0),
            ("f2", 240.0, 220.0, 60.0, 20.0),
            ("g", 0.0, 264.0, 300.0, 40.0),
            ("g1", 0.0, 264.0, 60.0, 20.0),
            ("g2", 60.0, 264.0, 150.0, 20.0),
        ],
    },
    Case {
        name: "crossclamp",
        spec: "§9.4 stretch clamped by min/max cross size",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;width:300px;background:#eee;margin-bottom:4px}\n.c > div{background:#8cf}\n</style>\n<div class=c id=a style=\"height:60px\"><div id=a1 style=\"width:50px;max-height:20px\"></div><div id=a2 style=\"width:50px\"></div></div>\n<div class=c id=b style=\"height:60px\"><div id=b1 style=\"width:50px;min-height:80px\"></div></div>\n<div class=c id=c style=\"height:60px\"><div id=c1 style=\"width:50px;max-height:20px;align-self:center\"></div></div>\n<div class=c id=d style=\"flex-direction:column;height:60px\"><div id=d1 style=\"height:20px;max-width:40px\"></div></div>\n<div class=c id=e style=\"flex-direction:column;height:60px\"><div id=e1 style=\"height:20px;min-width:400px\"></div></div>\n<div class=c id=f style=\"height:60px;align-items:center\"><div id=f1 style=\"width:50px;height:20px;margin-top:10px;margin-bottom:6px\"></div></div>\n<div class=c id=g style=\"height:60px\"><div id=g1 style=\"width:50px;margin-top:10px;margin-bottom:6px\"></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 300.0, 60.0),
            ("a1", 0.0, 0.0, 50.0, 20.0),
            ("a2", 50.0, 0.0, 50.0, 60.0),
            ("b", 0.0, 64.0, 300.0, 60.0),
            ("b1", 0.0, 64.0, 50.0, 80.0),
            ("c", 0.0, 128.0, 300.0, 60.0),
            ("c1", 0.0, 158.0, 50.0, 0.0),
            ("d", 0.0, 192.0, 300.0, 60.0),
            ("d1", 0.0, 192.0, 40.0, 20.0),
            ("e", 0.0, 256.0, 300.0, 60.0),
            ("e1", 0.0, 256.0, 400.0, 20.0),
            ("f", 0.0, 320.0, 300.0, 60.0),
            ("f1", 0.0, 342.0, 50.0, 20.0),
            ("g", 0.0, 384.0, 300.0, 60.0),
            ("g1", 0.0, 394.0, 50.0, 44.0),
        ],
    },
    Case {
        name: "automin",
        spec: "§4.5 the automatic minimum size of a flex item",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;width:100px;height:40px;background:#eee;margin-bottom:4px}\n.c > div{background:#8cf}\n.w90 > i{display:block;width:90px;height:20px;background:#fa8}\n.w20 > i{display:block;width:20px;height:20px;background:#fa8}\n</style>\n<div class=c id=a><div id=a1 class=w90 style=\"flex:1 1 0;max-width:40px\"><i></i></div><div id=a2 class=w20 style=\"flex:1 1 0\"><i></i></div></div>\n<div class=c id=b><div id=b1 class=w90 style=\"flex:1 1 0\"><i></i></div><div id=b2 class=w20 style=\"flex:1 1 0\"><i></i></div></div>\n<div class=c id=c><div id=c1 class=w90 style=\"flex:1 1 0;width:30px\"><i></i></div><div id=c2 class=w20 style=\"flex:1 1 0\"><i></i></div></div>\n<div class=c id=d><div id=d1 class=w90 style=\"flex:1 1 0;width:30px;flex-basis:70px\"><i></i></div><div id=d2 class=w20 style=\"flex:1 1 0\"><i></i></div></div>\n<div class=c id=e><div id=e1 class=w90 style=\"flex:1 1 0;aspect-ratio:2;height:30px\"><i></i></div><div id=e2 class=w20 style=\"flex:1 1 0\"><i></i></div></div>\n<div class=c id=f><div id=f1 class=w90 style=\"flex:1 1 0;overflow:hidden\"><i></i></div><div id=f2 class=w20 style=\"flex:1 1 0\"><i></i></div></div>\n<div class=c id=g style=\"flex-direction:column;height:60px;width:100px\"><div id=g1 style=\"flex:1 1 0;max-height:15px\"><i style=\"display:block;height:40px;width:20px\"></i></div><div id=g2 style=\"flex:1 1 0\"><i style=\"display:block;height:5px;width:20px\"></i></div></div>\n<div class=c id=h style=\"flex-direction:column;height:60px;width:100px\"><div id=h1 style=\"flex:1 1 0\"><i style=\"display:block;height:40px;width:20px\"></i></div><div id=h2 style=\"flex:1 1 0\"><i style=\"display:block;height:5px;width:20px\"></i></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 100.0, 40.0),
            ("a1", 0.0, 0.0, 40.0, 40.0),
            ("a2", 40.0, 0.0, 60.0, 40.0),
            ("b", 0.0, 44.0, 100.0, 40.0),
            ("b1", 0.0, 44.0, 90.0, 40.0),
            ("b2", 90.0, 44.0, 20.0, 40.0),
            ("c", 0.0, 88.0, 100.0, 40.0),
            ("c1", 0.0, 88.0, 50.0, 40.0),
            ("c2", 50.0, 88.0, 50.0, 40.0),
            ("d", 0.0, 132.0, 100.0, 40.0),
            ("d1", 0.0, 132.0, 80.0, 40.0),
            ("d2", 80.0, 132.0, 20.0, 40.0),
            ("e", 0.0, 176.0, 100.0, 40.0),
            ("e1", 0.0, 176.0, 90.0, 30.0),
            ("e2", 90.0, 176.0, 20.0, 40.0),
            ("f", 0.0, 220.0, 100.0, 40.0),
            ("f1", 0.0, 220.0, 50.0, 40.0),
            ("f2", 50.0, 220.0, 50.0, 40.0),
            ("g", 0.0, 264.0, 100.0, 60.0),
            ("g1", 0.0, 264.0, 100.0, 15.0),
            ("g2", 0.0, 279.0, 100.0, 45.0),
            ("h", 0.0, 328.0, 100.0, 60.0),
            ("h1", 0.0, 328.0, 100.0, 40.0),
            ("h2", 0.0, 368.0, 100.0, 20.0),
        ],
    },
    Case {
        name: "automin2",
        spec: "§4.5 specified/transferred size suggestions",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;width:100px;height:40px;background:#eee;margin-bottom:4px}\n.c > div{background:#8cf}\n.w90 > i{display:block;width:90px;height:20px;background:#fa8}\n</style>\n<div class=c id=a><div id=a1 class=w90 style=\"flex:0 1 200px;width:30px\"><i></i></div><div id=a2 style=\"flex:0 1 200px;min-width:0\"></div></div>\n<div class=c id=b><div id=b1 class=w90 style=\"flex:0 1 200px\"><i></i></div><div id=b2 style=\"flex:0 1 200px;min-width:0\"></div></div>\n<div class=c id=c><div id=c1 class=w90 style=\"flex:0 1 200px;max-width:45px\"><i></i></div><div id=c2 style=\"flex:0 1 200px;min-width:0\"></div></div>\n<div class=c id=d style=\"flex-direction:column;height:60px\"><div id=d1 style=\"flex:0 1 100px;height:15px\"><i style=\"display:block;height:40px;width:20px\"></i></div><div id=d2 style=\"flex:0 1 100px;min-height:0\"></div></div>\n<div class=c id=e><div id=e1 class=w90 style=\"flex:0 1 200px;box-sizing:border-box;padding-left:10px;width:30px\"><i></i></div><div id=e2 style=\"flex:0 1 200px;min-width:0\"></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 100.0, 40.0),
            ("a1", 0.0, 0.0, 50.0, 40.0),
            ("a2", 50.0, 0.0, 50.0, 40.0),
            ("b", 0.0, 44.0, 100.0, 40.0),
            ("b1", 0.0, 44.0, 90.0, 40.0),
            ("b2", 90.0, 44.0, 10.0, 40.0),
            ("c", 0.0, 88.0, 100.0, 40.0),
            ("c1", 0.0, 88.0, 45.0, 40.0),
            ("c2", 45.0, 88.0, 55.0, 40.0),
            ("d", 0.0, 132.0, 100.0, 60.0),
            ("d1", 0.0, 132.0, 100.0, 30.0),
            ("d2", 0.0, 162.0, 100.0, 30.0),
            ("e", 0.0, 196.0, 100.0, 40.0),
            ("e1", 0.0, 196.0, 53.84, 40.0),
            ("e2", 53.84, 196.0, 46.16, 40.0),
        ],
    },
    Case {
        name: "intrinsic",
        spec: "§9.9 intrinsic sizing of a flex container",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.o{background:#eee;margin-bottom:4px;width:400px}\n.f{display:flex;background:#cfc}\n.f > div{background:#8cf;height:20px}\n</style>\n<div class=o id=a><div class=f id=a0 style=\"width:max-content\"><div id=a1 style=\"width:60px\"></div><div id=a2 style=\"width:40px\"></div></div></div>\n<div class=o id=b><div class=f id=b0 style=\"width:min-content\"><div id=b1 style=\"width:60px\"></div><div id=b2 style=\"width:40px\"></div></div></div>\n<div class=o id=c><div class=f id=c0 style=\"width:min-content;flex-wrap:wrap\"><div id=c1 style=\"width:60px\"></div><div id=c2 style=\"width:40px\"></div></div></div>\n<div class=o id=d><div class=f id=d0 style=\"width:max-content\"><div id=d1 style=\"flex:1 1 0;width:60px\"></div><div id=d2 style=\"width:40px\"></div></div></div>\n<div class=o id=e><div class=f id=e0 style=\"width:max-content;gap:10px\"><div id=e1 style=\"width:60px\"></div><div id=e2 style=\"width:40px\"></div></div></div>\n<div class=o id=f><div class=f id=f0 style=\"width:max-content;flex-direction:column\"><div id=f1 style=\"width:60px\"></div><div id=f2 style=\"width:40px\"></div></div></div>\n<div class=o id=g><div class=f id=g0 style=\"display:inline-flex\"><div id=g1 style=\"width:60px\"></div><div id=g2 style=\"width:40px\"></div></div></div>\n<div class=o id=h><div class=f id=h0 style=\"float:left\"><div id=h1 style=\"width:60px\"></div><div id=h2 style=\"width:40px\"></div></div></div>\n<div class=o id=i style=\"clear:both\"><div class=f id=i0 style=\"width:max-content\"><div id=i1 style=\"flex:0 0 30px;min-width:70px\"></div><div id=i2 style=\"width:40px\"></div></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 400.0, 20.0),
            ("a0", 0.0, 0.0, 100.0, 20.0),
            ("a1", 0.0, 0.0, 60.0, 20.0),
            ("a2", 60.0, 0.0, 40.0, 20.0),
            ("b", 0.0, 24.0, 400.0, 20.0),
            ("b0", 0.0, 24.0, 100.0, 20.0),
            ("b1", 0.0, 24.0, 60.0, 20.0),
            ("b2", 60.0, 24.0, 40.0, 20.0),
            ("c", 0.0, 48.0, 400.0, 40.0),
            ("c0", 0.0, 48.0, 60.0, 40.0),
            ("c1", 0.0, 48.0, 60.0, 20.0),
            ("c2", 0.0, 68.0, 40.0, 20.0),
            ("d", 0.0, 92.0, 400.0, 20.0),
            ("d0", 0.0, 92.0, 100.0, 20.0),
            ("d1", 0.0, 92.0, 60.0, 20.0),
            ("d2", 60.0, 92.0, 40.0, 20.0),
            ("e", 0.0, 116.0, 400.0, 20.0),
            ("e0", 0.0, 116.0, 110.0, 20.0),
            ("e1", 0.0, 116.0, 60.0, 20.0),
            ("e2", 70.0, 116.0, 40.0, 20.0),
            ("f", 0.0, 140.0, 400.0, 40.0),
            ("f0", 0.0, 140.0, 60.0, 40.0),
            ("f1", 0.0, 140.0, 60.0, 20.0),
            ("f2", 0.0, 160.0, 40.0, 20.0),
            ("g", 0.0, 184.0, 400.0, 20.0),
            ("g0", 0.0, 184.0, 100.0, 20.0),
            ("g1", 0.0, 184.0, 60.0, 20.0),
            ("g2", 60.0, 184.0, 40.0, 20.0),
            ("h", 0.0, 208.0, 400.0, 0.0),
            ("h0", 0.0, 208.0, 100.0, 20.0),
            ("h1", 0.0, 208.0, 60.0, 20.0),
            ("h2", 60.0, 208.0, 40.0, 20.0),
            ("i", 0.0, 228.0, 400.0, 20.0),
            ("i0", 0.0, 228.0, 110.0, 20.0),
            ("i1", 0.0, 228.0, 70.0, 20.0),
            ("i2", 70.0, 228.0, 40.0, 20.0),
        ],
    },
    Case {
        name: "pct",
        spec: "§9.8 percentages against an indefinite container size",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.c{display:flex;background:#eee;margin-bottom:4px;width:300px}\n.c > div{background:#8cf}\n</style>\n<div class=c id=a><div id=a1 style=\"width:50px;height:50%\"></div><div id=a2 style=\"width:50px;height:40px\"></div></div>\n<div class=c id=b style=\"height:200px\"><div id=b1 style=\"width:50px;height:50%\"></div></div>\n<div class=c id=c style=\"flex-direction:column\"><div id=c1 style=\"height:20px;flex-basis:50%\"></div><div id=c2 style=\"height:20px\"></div></div>\n<div class=c id=d style=\"flex-direction:column;height:200px\"><div id=d1 style=\"flex-basis:25%\"></div><div id=d2 style=\"height:20px\"></div></div>\n<div class=c id=e><div id=e1 style=\"width:50%;flex:0 0 auto\"></div><div id=e2 style=\"width:20px;height:20px\"></div></div>\n<div class=c id=f style=\"flex-direction:column\"><div id=f1 style=\"height:20px;padding-top:10%\"></div></div>\n<div class=c id=g><div id=g1 style=\"flex:0 0 50%;height:20px;margin-left:10%\"></div></div>\n<div class=c id=h style=\"flex-direction:column;height:100px\"><div id=h1 style=\"flex:1 1 auto;min-height:0\"><div id=h2 style=\"height:50%;width:20px;background:#fa8\"></div></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 300.0, 40.0),
            ("a1", 0.0, 0.0, 50.0, 0.0),
            ("a2", 50.0, 0.0, 50.0, 40.0),
            ("b", 0.0, 44.0, 300.0, 200.0),
            ("b1", 0.0, 44.0, 50.0, 100.0),
            ("c", 0.0, 248.0, 300.0, 20.0),
            ("c1", 0.0, 248.0, 300.0, 0.0),
            ("c2", 0.0, 248.0, 300.0, 20.0),
            ("d", 0.0, 272.0, 300.0, 200.0),
            ("d1", 0.0, 272.0, 300.0, 50.0),
            ("d2", 0.0, 322.0, 300.0, 20.0),
            ("e", 0.0, 476.0, 300.0, 20.0),
            ("e1", 0.0, 476.0, 150.0, 20.0),
            ("e2", 150.0, 476.0, 20.0, 20.0),
            ("f", 0.0, 500.0, 300.0, 50.0),
            ("f1", 0.0, 500.0, 300.0, 50.0),
            ("g", 0.0, 554.0, 300.0, 20.0),
            ("g1", 30.0, 554.0, 150.0, 20.0),
            ("h", 0.0, 578.0, 300.0, 100.0),
            ("h1", 0.0, 578.0, 300.0, 100.0),
            ("h2", 0.0, 578.0, 20.0, 50.0),
        ],
    },
    Case {
        name: "frac2",
        spec: "§9.7 flex factors below one, WPT flex-factor-less-than-one shapes",
        html: "<style>\nbody{margin:0;font:16px/20px monospace}\n.f{display:flex}\n.c{height:100px;width:100px;border:1px solid black;margin-bottom:4px}\n</style>\n<div class=\"f c\" id=a><div id=a1 style=\"flex-grow:0.5\"></div></div>\n<div class=\"f c\" id=b><div id=b1 style=\"flex-grow:0.5\"></div><div id=b2 style=\"flex-grow:0.25\"></div></div>\n<div class=\"f c\" id=c><div id=c1 style=\"flex-grow:0.5;flex-basis:30px\"></div><div id=c2 style=\"flex-grow:0.25;flex-basis:30px\"></div></div>\n<div class=\"f c\" id=d><div id=d1 style=\"flex-shrink:0.5;width:200px;height:200px\"></div></div>\n<div class=\"f c\" id=e><div id=e1 style=\"flex-shrink:0.5;width:200px;height:200px\"></div><div id=e2 style=\"flex-shrink:0.25;width:200px;height:200px\"></div></div>\n<div class=\"f c\" id=f style=\"flex-direction:column\"><div id=f1 style=\"flex-grow:0.5\"></div></div>\n<div class=\"f c\" id=g style=\"flex-direction:column\"><div id=g1 style=\"flex-grow:0.75;flex-basis:100px\"></div><div id=g2 style=\"flex-grow:0.25;flex-basis:100px\"></div></div>",
        expect: &[
            ("a", 0.0, 0.0, 102.0, 102.0),
            ("a1", 1.0, 1.0, 50.0, 100.0),
            ("b", 0.0, 106.0, 102.0, 102.0),
            ("b1", 1.0, 107.0, 50.0, 100.0),
            ("b2", 51.0, 107.0, 25.0, 100.0),
            ("c", 0.0, 212.0, 102.0, 102.0),
            ("c1", 1.0, 213.0, 50.0, 100.0),
            ("c2", 51.0, 213.0, 40.0, 100.0),
            ("d", 0.0, 318.0, 102.0, 102.0),
            ("d1", 1.0, 319.0, 150.0, 200.0),
            ("e", 0.0, 424.0, 102.0, 102.0),
            ("e1", 1.0, 425.0, 50.0, 200.0),
            ("e2", 51.0, 425.0, 125.0, 200.0),
            ("f", 0.0, 530.0, 102.0, 102.0),
            ("f1", 1.0, 531.0, 100.0, 50.0),
            ("g", 0.0, 636.0, 102.0, 102.0),
            ("g1", 1.0, 637.0, 100.0, 50.0),
            ("g2", 1.0, 687.0, 100.0, 50.0),
        ],
    },
];

/// Every case, reported together — one failing element should not hide the
/// rest, because a layout change usually moves several at once.
#[test]
fn flex_compliance_corpus_matches_a_browser() {
    // A whole pixel: the expectations are a browser's rounded values, and a
    // half-pixel of leading is not a compliance failure.
    const TOL: f32 = 1.0;
    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for case in CASES {
        let mut r = Renderer::new();
        let mut doc = r.load_html_vp(case.html, VIEWPORT_W, VIEWPORT_H);
        for (id, ex, ey, ew, eh) in case.expect {
            let Some(node) = doc.get_element_by_id(id) else {
                failures.push(format!("{}: #{} not found", case.name, id));
                continue;
            };
            let Some(b) = doc.get_bounding_client_rect(node) else {
                failures.push(format!("{}: #{} has no box", case.name, id));
                continue;
            };
            checked += 1;
            if (b.x - ex).abs() > TOL
                || (b.y - ey).abs() > TOL
                || (b.w - ew).abs() > TOL
                || (b.h - eh).abs() > TOL
            {
                failures.push(format!(
                    "{} [{}]: #{} is {}x{} at {},{} — a browser gives {}x{} at {},{}",
                    case.name,
                    case.spec,
                    id,
                    b.w.round(),
                    b.h.round(),
                    b.x.round(),
                    b.y.round(),
                    ew,
                    eh,
                    ex,
                    ey
                ));
            }
        }
    }

    assert!(
        checked > 300,
        "the corpus should cover 300+ boxes, checked {checked}"
    );
    assert!(
        failures.is_empty(),
        "{} of {} boxes disagree with a browser:\n  {}",
        failures.len(),
        checked,
        failures.join("\n  ")
    );
}

#[test]
fn rtl_wrapping_flex_row_keeps_physical_margin_gutters() {
    let html = r#"
        <style>
            body { margin: 0; }
            .row {
                direction: rtl;
                display: flex;
                flex-wrap: wrap;
                width: 1008px;
            }
            .row > div {
                width: 240px;
                height: 20px;
                flex: 0 0 auto;
                margin-left: 16px;
            }
            .row > div:last-child { margin-left: 0; }
        </style>
        <div class="row">
            <div id="a"></div>
            <div id="b"></div>
            <div id="c"></div>
            <div id="d"></div>
        </div>
    "#;
    let mut r = Renderer::new();
    let mut doc = r.load_html_vp(html, VIEWPORT_W, VIEWPORT_H);
    let a = doc.query_selector("#a").unwrap();
    let b = doc.query_selector("#b").unwrap();
    let c = doc.query_selector("#c").unwrap();
    let d = doc.query_selector("#d").unwrap();

    assert!((doc.offset_left(a) - 768.0).abs() < 0.5);
    assert!((doc.offset_left(b) - 512.0).abs() < 0.5);
    assert!((doc.offset_left(c) - 256.0).abs() < 0.5);
    assert!((doc.offset_left(d) - 0.0).abs() < 0.5);
}

// TEMPORARY DIAGNOSTIC — remove before finishing.
#[test]
fn zz_probe_one_wpt() {
    let name = std::env::var("WPTONE").unwrap_or_default();
    if name.is_empty() {
        return;
    }
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/wpt");
    let path = root.join("css-flexbox").join(&name);
    let raw = std::fs::read_to_string(&path).unwrap();
    let base = format!("file://{}", root.display());
    let html = raw
        .replace("href=\"/", &format!("href=\"{base}/"))
        .replace("src=\"/", &format!("src=\"{base}/"));
    let pbase = format!("file://{}", path.parent().unwrap().display());
    let mut r = crate::Renderer::new();
    let mut doc = r.load_html_with_base(&html, &pbase, 800.0, 600.0);
    for attr in [
        "data-expected-width",
        "data-expected-height",
        "data-offset-x",
        "data-offset-y",
    ] {
        for id in crate::dom::query_selector_all_ids(&doc.root, &format!("[{attr}]")) {
            let want: f32 = doc.get_attribute(id, attr).unwrap().trim().parse().unwrap();
            let got = match attr {
                "data-expected-width" => doc.offset_width(id),
                "data-expected-height" => doc.offset_height(id),
                "data-offset-x" => doc.offset_left(id),
                _ => doc.offset_top(id),
            };
            let mark = if (got - want).abs() > 1.0 {
                "FAIL"
            } else {
                "ok  "
            };
            eprintln!("{mark} #{id} {attr} got {got} want {want}");
        }
    }
}

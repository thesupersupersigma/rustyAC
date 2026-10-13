// SPDX-License-Identifier: GPL-3.0-or-later

//! The GUI oracle's scripts: what both sides build, the mouse and key events they are fed and
//! the frames they draw. A script is a list of [`Op`]s; objects are named by the order they
//! were made in (the game screen is object 0).

#[derive(Clone, Debug)]
pub enum Val {
    F(f32),
    B(bool),
    I(i32),
    C([f32; 4]),
}

#[derive(Clone, Debug)]
pub enum Op {
    /// a new object of a class of ksgui: class, name, a flag for the classes that take one
    New(&'static str, &'static str, bool),
    /// the n-th child of an object becomes an object of its own
    Child(usize, usize),
    /// `GameScreen::addControl(control, showInTaskbar)` on the game screen
    AddScreen(usize, bool),
    /// `parent->addControl(child)`
    Add(usize, usize),
    Pos(usize, f32, f32),
    Size(usize, f32, f32),
    Text(usize, &'static str),
    Visible(usize, bool),
    /// a member written directly (the names of [`FIELDS`])
    Set(usize, &'static str, Val),
    /// a method of the object's class (the names each side knows)
    Call(usize, &'static str, f32, &'static str),
    MouseMove(i32, i32),
    MouseDown(i32, i32),
    MouseUp(i32, i32),
    Wheel(i32, i32, f32),
    KeyDown(u32),
    KeyChar(u32),
    /// a font of its own for the text of [`Op::Blit`]: type (0 Segoe UI, 1 Consolas), scale, italic, bold
    Font(i32, f32, bool, bool),
    /// text drawn after the GUI in every frame from here on: font, x, y, text, size, align, colour, shadowed
    Blit(usize, f32, f32, &'static str, f32, i32, [f32; 4], bool),
    /// the text list is emptied
    ClearText,
    /// one frame is drawn (and compared): the frame's time
    Frame(f32),
    /// `GUI::serializeForms`: the layout file is written (and compared)
    Save,
}

/// Members a script may write: name, offset in the game's object, kind (f float, b bool, i int,
/// c four floats).
pub const FIELDS: [(&str, usize, char); 24] = [
    ("backColor", 0x38, 'c'),
    ("borderColor", 0x48, 'c'),
    ("foreColor", 0x58, 'c'),
    ("highlightTextColor", 0x68, 'c'),
    ("highlight", 0x78, 'b'),
    ("backTextureColor", 0x7c, 'c'),
    ("fontAlign", 0xc0, 'i'),
    ("drawBorder", 0x110, 'b'),
    ("drawBackground", 0x111, 'b'),
    ("repeatInterval", 0x114, 'f'),
    ("fontScale", 0x118, 'f'),
    ("label.maxChars", 0x178, 'i'),
    ("progress.max", 0x178, 'f'),
    ("progress.min", 0x17c, 'f'),
    ("progress.value", 0x180, 'f'),
    ("progress.vertical", 0x184, 'b'),
    ("progress.inverted", 0x185, 'b'),
    ("button.status", 0x1c0, 'i'),
    ("button.blanked", 0x1c4, 'b'),
    ("button.highlightOnCursorOver", 0x1c5, 'b'),
    ("button.isActive", 0x1c6, 'b'),
    ("slider.value", 0x1a8, 'f'),
    ("screen.appInteractionEnabled", 0x178, 'b'),
    ("form.devApp", 0x180, 'b'),
];

pub fn field(name: &str) -> Result<(usize, char), String> {
    FIELDS.iter().find(|f| f.0 == name).map(|f| (f.1, f.2)).ok_or(format!("no member {name:?} in the oracle's table"))
}

/// A layout file for the sequences that want forms to come up as a session left them.
pub const ACOS_SAMPLE: &str = "[HEADER]\nVERSION=2\nDESKTOP_SELECTED=1\n\n[DESK_0_FORM_ALPHA]\nPOSX=120\nPOSY=140\nVISIBLE=1\nBLOCKED=0\nSCALE=1\n\n[DESK_0_FORM_BETA]\nPOSX=520.5\nPOSY=300\nVISIBLE=1\nBLOCKED=1\nSCALE=1.25\n\n[DESK_1_FORM_ALPHA]\nPOSX=700\nPOSY=90\nVISIBLE=1\nBLOCKED=0\nSCALE=1.5\n\n[DESK_1_FORM_BETA]\nPOSX=5000\nPOSY=20\nVISIBLE=0\nBLOCKED=0\nSCALE=1\n\n[DESK_2_FORM_GAMMA]\nPOSX=40\nPOSY=5000\nVISIBLE=1\nBLOCKED=0\n\n";

pub struct Sequence {
    pub ops: Vec<Op>,
    /// `cfg/acos.ini` as the session finds it (none: no file)
    pub acos: Option<&'static str>,
    /// `cfg/gameplay.ini [GUI] ALLOW_OVERLAPPING_FORMS`
    pub allow_overlapping: bool,
}

const DT: f32 = 1.0 / 60.0;

fn frames(ops: &mut Vec<Op>, count: usize) {
    for _ in 0..count {
        ops.push(Op::Frame(DT));
    }
}

/// Plain controls and labels: colours, frames, text in the three alignments, children, a
/// highlighted text under the mouse, a held control that repeats its click, a scaled tree.
fn base() -> Vec<Op> {
    use Op::*;
    let mut o = vec![
        New("Control", "PANEL", false), // 1
        AddScreen(1, false),
        Pos(1, 100.0, 120.0),
        Size(1, 400.0, 220.0),
        Text(1, "A panel: Control"),
        New("Label", "First label", false), // 2
        Add(1, 2),
        Pos(2, 10.0, 40.0),
        Size(2, 200.0, 24.0),
        New("Control", "RIGHT", false), // 3
        Add(1, 3),
        Pos(3, 10.0, 80.0),
        Size(3, 380.0, 30.0),
        Text(3, "right aligned 0123456789"),
        Set(3, "fontAlign", Val::I(1)),
        Set(3, "backColor", Val::C([0.1, 0.2, 0.6, 0.9])),
        Set(3, "borderColor", Val::C([1.0, 0.8, 0.0, 1.0])),
        New("Control", "LEFT", false), // 4
        Add(1, 4),
        Pos(4, 10.0, 120.0),
        Size(4, 380.0, 30.0),
        Text(4, "left aligned, highlighted under the mouse"),
        Set(4, "fontAlign", Val::I(0)),
        Set(4, "highlight", Val::B(true)),
        Set(4, "highlightTextColor", Val::C([1.0, 0.0, 0.0, 1.0])),
        Set(4, "drawBorder", Val::B(false)),
        New("Label", "A label whose text is far too long for what it may show", false), // 5
        Add(1, 5),
        Pos(5, 10.0, 160.0),
        Size(5, 380.0, 24.0),
        Set(5, "label.maxChars", Val::I(18)),
        Set(5, "foreColor", Val::C([0.5, 1.0, 0.5, 0.75])),
        New("Control", "REPEATER", false), // 6
        AddScreen(6, false),
        Pos(6, 600.0, 120.0),
        Size(6, 120.0, 40.0),
        Text(6, "hold me"),
        Set(6, "repeatInterval", Val::F(0.25)),
        New("Control", "INNER", false), // 7
        Add(6, 7),
        Pos(7, 4.0, 22.0),
        Size(7, 60.0, 14.0),
        Set(7, "backColor", Val::C([0.0, 0.0, 0.0, 1.0])),
        Set(7, "fontScale", Val::F(0.6)),
        Text(7, "child"),
        Frame(DT),
        Frame(DT),
        MouseMove(200, 250),
        Frame(DT),
        MouseMove(50, 50),
        Frame(DT),
        MouseMove(640, 135),
        MouseDown(640, 135),
    ];
    // held for a second and a half: the click repeats
    frames(&mut o, 90);
    o.push(MouseUp(640, 135));
    frames(&mut o, 2);
    // clicks that hit a child and its parent, and one that hits nothing
    o.extend([MouseDown(110, 205), MouseUp(110, 205), Frame(DT), MouseDown(5, 5), MouseUp(5, 5), Frame(DT)]);
    // the panel and all in it at one and a half times the size, then at a half
    o.extend([Call(1, "scaleByMult", 1.5, ""), Frame(DT), Call(1, "scaleByMult", 0.5, ""), Frame(DT), Call(1, "scaleByMult", 1.0, ""), Frame(DT)]);
    // hidden, moved, shown
    o.extend([Visible(3, false), Frame(DT), Pos(1, 300.0, 300.0), Visible(3, true), Frame(DT), Visible(1, false), Frame(DT)]);
    o
}

/// Forms: one that can be scaled and one that cannot, the buttons that show under the mouse,
/// dragging (over another form: red, and back), pinning, zooming, the app bar sliding in, its
/// icons and their titles, the four desktops, "show desktop", and the layout file written.
fn forms() -> Vec<Op> {
    use Op::*;
    let mut o = vec![
        New("Form", "ALPHA", true), // 1
        Call(1, "title", 0.0, "Alpha app"),
        Size(1, 260.0, 160.0),
        AddScreen(1, true),
        New("Label", "Speed: 123 km/h", false), // 2
        Add(1, 2),
        Pos(2, 12.0, 40.0),
        Size(2, 200.0, 22.0),
        New("Form", "BETA", false), // 3
        Call(3, "title", 0.0, "Beta, not scalable"),
        Size(3, 300.0, 120.0),
        AddScreen(3, true),
        New("Form", "GAMMA", true), // 4
        Call(4, "title", 0.0, "Gamma"),
        Size(4, 180.0, 90.0),
        AddScreen(4, true),
        New("Form", "HIDDEN_DEV", true), // 5: not on the bar
        Size(5, 100.0, 50.0),
        AddScreen(5, false),
        Frame(DT),
        Frame(DT),
    ];
    // the mouse over the first form: pin and zoom show; a click on zoom in, twice, zoom out once
    o.extend([MouseMove(200, 200), Frame(DT)]);
    o
}

fn forms_more(o: &mut Vec<Op>) {
    use Op::*;
    // everything visible to begin with, wherever the layout file put it
    for id in [1, 3, 4] {
        o.push(Visible(id, true));
    }
    o.extend([Pos(1, 120.0, 140.0), Pos(3, 520.0, 300.0), Pos(4, 60.0, 420.0), Frame(DT)]);
    // hover the first form, press its zoom buttons (above its top edge, right to left: pin, out, in)
    o.extend([MouseMove(200, 200), Frame(DT)]);
    for (x, n) in [(120 + 260 - 1 - 26 - 26 - 13, 2), (120 + 260 - 1 - 26 - 13, 1)] {
        for _ in 0..n {
            o.extend([MouseMove(x, 126), MouseDown(x, 126), MouseUp(x, 126), Frame(DT)]);
        }
    }
    // drag it by its body, over the second form (red), let go there (it goes back), drag clear
    o.extend([MouseMove(200, 200), MouseDown(200, 200), Frame(DT)]);
    for step in 1..=12 {
        o.extend([MouseMove(200 + step * 30, 200 + step * 12), Frame(DT)]);
    }
    o.extend([MouseUp(560, 344), Frame(DT), Frame(DT)]);
    o.extend([MouseMove(200, 200), MouseDown(200, 200)]);
    for step in 1..=8 {
        o.extend([MouseMove(200 + step * 10, 200 - step * 20), Frame(DT)]);
    }
    o.extend([MouseUp(280, 40), Frame(DT)]);
    // pin the second form (its pin is inside, top right), then try to drag it
    o.extend([MouseMove(700, 320), Frame(DT), MouseMove(520 + 300 - 33 + 10, 300 + 10), MouseDown(520 + 300 - 33 + 10, 300 + 10), MouseUp(520 + 300 - 33 + 10, 300 + 10), Frame(DT)]);
    o.extend([MouseMove(600, 380), MouseDown(600, 380), MouseMove(640, 420), Frame(DT), MouseMove(700, 480), Frame(DT), MouseUp(700, 480), Frame(DT)]);
    // auto-hide on the pinned form: frame and title only under the mouse
    o.extend([Call(3, "autohide", 1.0, ""), Frame(DT), MouseMove(100, 700), Frame(DT), MouseMove(600, 380), Frame(DT)]);
    // the logo closes a form
    o.extend([MouseMove(70, 430), MouseDown(70, 430), MouseUp(70, 430), Frame(DT)]);
    // the app bar: the mouse at the right edge calls it in; it slides for a quarter of a second
    o.push(MouseMove(1270, 300));
    frames(o, 24);
    // over an icon: its title slides out; click: the form shows again; again: it hides
    o.push(MouseMove(1250, 60));
    frames(o, 22);
    o.extend([MouseDown(1250, 60), MouseUp(1250, 60), Frame(DT), Frame(DT), MouseMove(1250, 130), Frame(DT), Frame(DT), MouseDown(1250, 130), MouseUp(1250, 130), Frame(DT)]);
    // the wheel on the bar, the scroll buttons
    o.extend([Wheel(1250, 300, -120.0), Frame(DT), Wheel(1250, 300, 120.0), Frame(DT), MouseMove(1250, 10), MouseDown(1250, 10), MouseUp(1250, 10), Frame(DT), MouseMove(1250, 700), MouseDown(1250, 700), MouseUp(1250, 700), Frame(DT)]);
    // the desktops: the second button, the fourth, back to the first
    for x in [1216 + 18 + 7, 1216 + 48 + 7, 1216 + 2 + 7] {
        o.extend([MouseMove(x, 686), MouseDown(x, 686), MouseUp(x, 686), Frame(DT), Frame(DT)]);
    }
    // the key's way round the desktops
    o.extend([Call(0, "cycleDesktop", 0.0, ""), Frame(DT), Call(0, "cycleDesktop", 0.0, ""), Frame(DT), Call(0, "cycleDesktop", 0.0, ""), Call(0, "cycleDesktop", 0.0, ""), Frame(DT)]);
    // the mouse leaves: the bar slides out
    o.push(MouseMove(400, 600));
    frames(o, 24);
    // "show desktop": everything hides, and comes back
    o.extend([Call(0, "showDesktop", 0.0, ""), Frame(DT), Call(0, "showDesktop", 0.0, ""), Frame(DT)]);
    o.push(Save);
}

/// The first widgets: progress bars both ways, buttons in their states, check boxes, the
/// replay's slider with its markers.
fn widgets_a() -> Vec<Op> {
    use Op::*;
    let mut o = vec![
        New("Form", "WIDGETS", true), // 1
        Call(1, "title", 0.0, "Widgets"),
        Size(1, 700.0, 420.0),
        AddScreen(1, true),
        Visible(1, true),
        Pos(1, 60.0, 120.0),
        New("ProgressBar", "PB_V", false), // 2
        Add(1, 2),
        Pos(2, 20.0, 50.0),
        Size(2, 20.0, 120.0),
        Set(2, "progress.value", Val::F(0.3)),
        New("ProgressBar", "PB_H", false), // 3
        Add(1, 3),
        Pos(3, 60.0, 50.0),
        Size(3, 200.0, 18.0),
        Set(3, "progress.vertical", Val::B(false)),
        Set(3, "progress.value", Val::F(0.8)),
        New("ProgressBar", "PB_HI", false), // 4
        Add(1, 4),
        Pos(4, 60.0, 80.0),
        Size(4, 200.0, 18.0),
        Set(4, "progress.vertical", Val::B(false)),
        Set(4, "progress.inverted", Val::B(true)),
        Set(4, "progress.max", Val::F(200.0)),
        Set(4, "progress.min", Val::F(-100.0)),
        Set(4, "progress.value", Val::F(250.0)),
        Text(4, "over the top"),
        New("ActiveButton", "BUTTON_PLAIN", false), // 5
        Add(1, 5),
        Pos(5, 60.0, 120.0),
        Size(5, 120.0, 30.0),
        Text(5, "Plain button"),
        New("ActiveButton", "BUTTON_TEX", false), // 6
        Add(1, 6),
        Pos(6, 200.0, 120.0),
        Size(6, 40.0, 40.0),
        Call(6, "multipleTexture", 0.0, "content/gui/controls/form/pin_icn"),
        New("ActiveButton", "BUTTON_OFF", false), // 7
        Add(1, 7),
        Pos(7, 260.0, 120.0),
        Size(7, 120.0, 30.0),
        Text(7, "Inactive"),
        Set(7, "button.isActive", Val::B(false)),
        New("ActiveButton", "BUTTON_MISSING", false), // 8
        Add(1, 8),
        Pos(8, 400.0, 120.0),
        Size(8, 40.0, 40.0),
        Call(8, "multipleTexture", 0.0, "content/gui/controls/form/there_is_no_such"),
        Text(8, "blank"),
        Set(8, "button.blanked", Val::B(true)),
        New("CheckBox", "Check me", false), // 9
        Add(1, 9),
        Pos(9, 60.0, 180.0),
        New("CheckBox", "Checked", false), // 10
        Add(1, 10),
        Pos(10, 60.0, 215.0),
        Call(10, "setCheck", 1.0, ""),
        New("Slider", "SLIDER", true), // 11
        Add(1, 11),
        Pos(11, 60.0, 290.0),
        Size(11, 500.0, 12.0),
        Set(11, "slider.value", Val::F(0.35)),
        New("Slider", "SLIDER_BARE", false), // 12
        Add(1, 12),
        Pos(12, 60.0, 340.0),
        Size(12, 300.0, 10.0),
        Frame(DT),
        Frame(DT),
    ];
    // hover the buttons, select one, click the check boxes (the box, then the label)
    for (x, y) in [(180, 255), (280, 260), (380, 255), (480, 260)] {
        o.extend([MouseMove(x, y), Frame(DT)]);
    }
    o.extend([Call(5, "setSelected", 1.0, ""), Frame(DT), Call(5, "setSelected", 0.0, ""), Call(6, "setSelected", 1.0, ""), Frame(DT)]);
    o.extend([MouseMove(130, 312), MouseDown(130, 312), MouseUp(130, 312), Frame(DT), MouseDown(130, 312), MouseUp(130, 312), Frame(DT), MouseMove(200, 312), MouseDown(200, 312), MouseUp(200, 312), Frame(DT), MouseMove(128, 345), MouseDown(128, 345), MouseUp(128, 345), Frame(DT)]);
    o.extend([Text(9, "A new text for the box"), Frame(DT)]);
    // the slider: a click moves the time, the markers are dragged, the cut is told
    o.extend([MouseMove(300, 415), MouseDown(300, 415), MouseUp(300, 415), Frame(DT)]);
    o.extend([MouseMove(112, 414), MouseDown(112, 414), MouseMove(200, 414), Frame(DT), MouseMove(260, 420), Frame(DT), MouseUp(260, 420), Frame(DT)]);
    o.extend([MouseMove(626, 414), MouseDown(626, 414), MouseMove(500, 414), Frame(DT), MouseMove(100, 414), Frame(DT), MouseUp(100, 414), Frame(DT)]);
    o.extend([Call(11, "markerLeft", 0.2, ""), Call(11, "markerRight", 0.9, ""), Frame(DT), Call(11, "updateMarkers", 0.0, ""), Frame(DT), Call(11, "resetMarkers", 0.0, ""), Frame(DT)]);
    // all of it scaled by the form's zoom
    o.extend([Call(1, "scaleByMult", 1.25, ""), Frame(DT), Call(1, "scaleByMult", 0.75, ""), Frame(DT)]);
    o
}

/// Text alone: both families, bold and italic, sizes from tiny to huge, the three alignments,
/// a shadow, a long string, characters beyond Latin.
fn text() -> Vec<Op> {
    use Op::*;
    let white = [1.0, 1.0, 1.0, 1.0];
    let mut o = vec![
        Font(0, 1.0, false, false), // 0
        Font(1, 1.0, false, false), // 1
        Font(0, 1.0, false, true),  // 2
        Font(0, 1.0, true, false),  // 3
        Font(1, 1.0, true, true),   // 4
        Font(0, 2.0, false, false), // 5: a font with a scale of its own
    ];
    let line = "Sphinx of black quartz, judge my vow 0123456789 AVWAT fi ffl";
    for (i, size) in [6.0f32, 9.0, 12.0, 16.0, 22.0, 30.0, 44.0].into_iter().enumerate() {
        o.push(Blit(i % 5, 20.0, 20.0 + 48.0 * i as f32, line, size, 0, [1.0, 1.0 - 0.1 * i as f32, 0.4, 1.0], false));
    }
    o.extend([Frame(DT), ClearText]);
    o.extend([
        Blit(0, 640.0, 40.0, "left of the point", 20.0, 0, white, false),
        Blit(0, 640.0, 70.0, "right of the point", 20.0, 1, white, false),
        Blit(0, 640.0, 100.0, "centred on the point", 20.0, 2, white, false),
        Blit(2, 100.0, 160.0, "A shadow under bold text", 36.0, 0, [1.0, 0.9, 0.2, 1.0], true),
        Blit(1, 100.0, 220.0, "half transparent, with its shadow", 24.0, 0, [0.4, 0.8, 1.0, 0.5], true),
        Blit(5, 100.0, 270.0, "a font with scale 2 at size 14", 14.0, 0, white, false),
        Blit(0, 20.0, 330.0, "A very long string that runs off the right edge of the screen without being wrapped anywhere, because the game draws every string with word wrap switched off, however long it is: 0123456789 0123456789 0123456789 0123456789 0123456789", 18.0, 0, white, false),
        Blit(0, 20.0, 380.0, "Wide: \u{e0}\u{e8}\u{ec}\u{f2}\u{f9} \u{c4}\u{d6}\u{dc}\u{df} \u{f1} \u{20ac} \u{3a9}\u{3c0} \u{416}\u{434}\u{44f} \u{65e5}\u{672c}\u{8a9e} \u{d55c}\u{ae00} \u{2603}", 28.0, 0, white, false),
        Blit(3, 20.0, 440.0, "Italic \u{2014} with a dash, \u{201c}quotes\u{201d} and an ellipsis\u{2026}", 26.0, 0, white, false),
        Blit(4, 20.0, 500.0, "Consolas bold italic 0O1lI| {}[]()<>", 26.0, 0, [0.6, 1.0, 0.6, 1.0], false),
        Blit(0, 20.0, 560.0, "", 26.0, 0, white, false),
        Blit(0, 20.0, 600.0, "two\nlines", 20.0, 0, white, false),
        Frame(DT),
        Frame(DT),
    ]);
    o
}

pub fn sequence(name: &str) -> Result<Sequence, String> {
    let plain = |ops: Vec<Op>| Sequence { ops, acos: None, allow_overlapping: false };
    Ok(match name {
        "base" => plain(base()),
        "text" => plain(text()),
        "widgets_a" => plain(widgets_a()),
        // forms as a first session finds them (no layout file)
        "forms" => {
            let mut ops = forms();
            forms_more(&mut ops);
            plain(ops)
        }
        // … as a layout file left them, on its second desktop
        "forms_layout" => {
            let mut ops = forms();
            ops.extend([Op::Frame(DT), Op::Call(0, "cycleDesktop", 0.0, ""), Op::Frame(DT), Op::Call(0, "cycleDesktop", 0.0, ""), Op::Frame(DT), Op::Save]);
            Sequence { ops, acos: Some(ACOS_SAMPLE), allow_overlapping: false }
        }
        // … and with forms allowed over each other
        "forms_overlap" => {
            let mut ops = forms();
            forms_more(&mut ops);
            Sequence { ops, acos: None, allow_overlapping: true }
        }
        other => return Err(format!("no GUI sequence {other:?} (base, text, forms, forms_layout, forms_overlap, widgets_a)")),
    })
}

/// What one side found, for the comparison.
#[derive(Default)]
pub struct Outcome {
    /// a line a frame: calls, draws, the hashes of the command log and of the picture
    pub frames: Vec<String>,
    /// the state of every control after every frame, and the events since the frame before
    pub state: String,
    /// the layout file of every `Save`
    pub saved: Vec<Vec<u8>>,
}

/// What both sides write down of a control (floats as their bits).
pub struct ControlState {
    pub class: &'static str,
    pub name: String,
    pub rect: [f32; 4],
    pub rect_base: [f32; 4],
    pub visible: bool,
    pub is_mouse_down: bool,
    pub highlight: bool,
    pub is_highlight: bool,
    pub draw_border: bool,
    pub draw_background: bool,
    pub text: String,
    pub font_scale: f32,
    pub scale_mult: f32,
    pub font_align: i32,
    /// back, border, fore, highlighted text, back texture
    pub colors: [[f32; 4]; 5],
    pub has_texture: bool,
    pub repeat: (f32, f32, bool),
    /// what the class adds
    pub extra: String,
}

pub fn bits4(v: &[f32; 4]) -> String {
    format!("{:08x},{:08x},{:08x},{:08x}", v[0].to_bits(), v[1].to_bits(), v[2].to_bits(), v[3].to_bits())
}

pub fn control_line(depth: usize, c: &ControlState) -> String {
    format!(
        "{}{} \"{}\" r={} rb={} vis={} md={} hl={}/{} draw={}{} text=\"{}\" fs={:08x} sm={:08x} al={} back={} border={} fore={} hlt={} btc={} tex={} rep={:08x},{:08x},{}{}
",
        "  ".repeat(depth),
        c.class,
        c.name,
        bits4(&c.rect),
        bits4(&c.rect_base),
        c.visible as u8,
        c.is_mouse_down as u8,
        c.highlight as u8,
        c.is_highlight as u8,
        c.draw_border as u8,
        c.draw_background as u8,
        c.text,
        c.font_scale.to_bits(),
        c.scale_mult.to_bits(),
        c.font_align,
        bits4(&c.colors[0]),
        bits4(&c.colors[1]),
        bits4(&c.colors[2]),
        bits4(&c.colors[3]),
        bits4(&c.colors[4]),
        c.has_texture as u8,
        c.repeat.0.to_bits(),
        c.repeat.1.to_bits(),
        c.repeat.2 as u8,
        c.extra,
    )
}

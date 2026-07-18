use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

const MAX_EVENTS_PER_NODE: usize = 128;
const MAX_MODEL_NODES: usize = 65_536;
const MAX_MODEL_DEPTH: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    Nil,
    Bool(bool),
    Integer(i64),
    Number(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<Data>),
    Map(BTreeMap<String, Data>),
    Resource(u64),
    Callback(u64),
}

impl Data {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Integer(value) => Some(*value as f64),
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        self.as_f64().and_then(|value| {
            (value.is_finite() && value.fract() == 0.0)
                .then_some(value)
                .and_then(|value| u32::try_from(value as i64).ok())
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Application,
    Window,
    Layout,
    Control,
    Timer,
}

#[derive(Debug, Clone)]
pub struct Common {
    pub visible: bool,
    pub enabled: bool,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub minimum_width: Option<f32>,
    pub minimum_height: Option<f32>,
    pub maximum_width: Option<f32>,
    pub maximum_height: Option<f32>,
    pub tooltip: String,
    pub style_class: String,
    pub accessible_name: String,
    pub accessible_description: String,
    pub focus_requested: bool,
    pub was_hovered: bool,
    pub was_focused: bool,
}

impl Default for Common {
    fn default() -> Self {
        Self {
            visible: true,
            enabled: true,
            width: None,
            height: None,
            minimum_width: None,
            minimum_height: None,
            maximum_width: None,
            maximum_height: None,
            tooltip: String::new(),
            style_class: String::new(),
            accessible_name: String::new(),
            accessible_description: String::new(),
            focus_requested: false,
            was_hovered: false,
            was_focused: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WindowState {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub minimum_width: f32,
    pub minimum_height: f32,
    pub maximum_width: Option<f32>,
    pub maximum_height: Option<f32>,
    pub resizable: bool,
    pub high_dpi: bool,
    pub maximized: bool,
    pub minimized: bool,
    pub fullscreen: bool,
    pub always_on_top: bool,
    pub centered: bool,
    pub icon: Option<Vec<u8>>,
    pub position: Option<[f32; 2]>,
    pub dpi: f32,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            title: "言窗应用".into(),
            width: 960.0,
            height: 640.0,
            minimum_width: 1.0,
            minimum_height: 1.0,
            maximum_width: None,
            maximum_height: None,
            resizable: true,
            high_dpi: true,
            maximized: false,
            minimized: false,
            fullscreen: false,
            always_on_top: false,
            centered: false,
            icon: None,
            position: None,
            dpi: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    Vertical,
    Horizontal,
    Grid,
    Stack,
    Scroll,
}

#[derive(Debug, Clone)]
pub struct LayoutState {
    pub kind: LayoutKind,
    pub columns: usize,
    pub spacing: f32,
    pub padding: f32,
    pub horizontal_alignment: String,
    pub vertical_alignment: String,
    pub grow: f32,
}

impl LayoutState {
    pub fn new(kind: LayoutKind) -> Self {
        Self {
            kind,
            columns: 2,
            spacing: 8.0,
            padding: 8.0,
            horizontal_alignment: "左".into(),
            vertical_alignment: "上".into(),
            grow: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum CanvasCommand {
    Clear([u8; 4]),
    Line {
        from: [f32; 2],
        to: [f32; 2],
        color: [u8; 4],
        width: f32,
    },
    Rectangle {
        minimum: [f32; 2],
        maximum: [f32; 2],
        color: [u8; 4],
        stroke: [u8; 4],
        stroke_width: f32,
        radius: f32,
    },
    Circle {
        center: [f32; 2],
        radius: f32,
        color: [u8; 4],
        stroke: [u8; 4],
        stroke_width: f32,
    },
    Text {
        position: [f32; 2],
        text: String,
        color: [u8; 4],
        size: f32,
    },
    Image {
        minimum: [f32; 2],
        maximum: [f32; 2],
        bytes: Vec<u8>,
        decoded_bytes: usize,
    },
    Clip {
        minimum: [f32; 2],
        maximum: [f32; 2],
    },
    Transform {
        translation: [f32; 2],
        scale: [f32; 2],
    },
}

impl CanvasCommand {
    pub fn memory_cost(&self) -> usize {
        const COMMAND_OVERHEAD: usize = 128;
        let payload = match self {
            Self::Text { text, .. } => text.len(),
            Self::Image {
                bytes,
                decoded_bytes,
                ..
            } => bytes.len().saturating_add(*decoded_bytes),
            _ => 0,
        };
        COMMAND_OVERHEAD.saturating_add(payload)
    }
}

#[derive(Debug, Clone)]
pub enum ControlKind {
    Text,
    Button,
    Input {
        multiline: bool,
        placeholder: String,
    },
    Checkbox,
    Radio {
        group: String,
    },
    Select {
        options: Vec<String>,
    },
    Slider {
        minimum: f64,
        maximum: f64,
    },
    Progress,
    Image {
        bytes: Vec<u8>,
        preserve_ratio: bool,
    },
    List {
        items: Vec<String>,
    },
    Separator,
    Tabs {
        tabs: Vec<String>,
    },
    Menu {
        items: Vec<String>,
    },
    Canvas {
        commands: Vec<CanvasCommand>,
        memory_bytes: usize,
    },
}

#[derive(Debug, Clone)]
pub struct ControlState {
    pub kind: ControlKind,
    pub text: String,
    pub selected: bool,
    pub value: f64,
    pub selected_index: usize,
    pub text_color: Option<[u8; 4]>,
    pub background_color: Option<[u8; 4]>,
    pub border_color: Option<[u8; 4]>,
    pub border_width: f32,
    pub corner_radius: f32,
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: u16,
    pub padding: f32,
    pub margin: f32,
}

impl ControlState {
    pub fn new(kind: ControlKind) -> Self {
        Self {
            kind,
            text: String::new(),
            selected: false,
            value: 0.0,
            selected_index: 0,
            text_color: None,
            background_color: None,
            border_color: None,
            border_width: 0.0,
            corner_radius: 4.0,
            font_family: String::new(),
            font_size: 14.0,
            font_weight: 400,
            padding: 4.0,
            margin: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TimerState {
    pub interval: Duration,
    pub repeating: bool,
    pub next: Instant,
    pub callback: Option<u64>,
    pub cancelled: bool,
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    Application { title: String, theme: String },
    Window(WindowState),
    Layout(LayoutState),
    Control(ControlState),
    Timer(TimerState),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: u64,
    pub public_handle: u64,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub common: Common,
    pub kind: NodeKind,
    pub events: BTreeMap<String, u64>,
}

#[derive(Debug, Default)]
pub struct Model {
    next_id: u64,
    pub nodes: BTreeMap<u64, Node>,
    pub roots: BTreeSet<u64>,
    pub exit_requested: bool,
    pub repaint_requested: bool,
}

impl Model {
    pub fn create(&mut self, parent: Option<u64>, kind: NodeKind) -> Result<u64, &'static str> {
        if self.nodes.len() >= MAX_MODEL_NODES {
            return Err("GUI_RESOURCE_LIMIT");
        }
        if let Some(parent) = parent
            && !self.nodes.contains_key(&parent)
        {
            return Err("GUI_RESOURCE_CLOSED");
        }
        let mut ancestor = parent;
        let mut parent_depth = 0;
        while let Some(id) = ancestor {
            let node = self.nodes.get(&id).ok_or("GUI_RESOURCE_CLOSED")?;
            parent_depth += 1;
            if parent_depth >= MAX_MODEL_DEPTH {
                return Err("GUI_RESOURCE_LIMIT");
            }
            ancestor = node.parent;
        }
        self.next_id = self.next_id.checked_add(1).ok_or("GUI_RESOURCE_LIMIT")?;
        let id = self.next_id;
        self.nodes.insert(
            id,
            Node {
                id,
                public_handle: 0,
                parent,
                children: Vec::new(),
                common: Common::default(),
                kind,
                events: BTreeMap::new(),
            },
        );
        if let Some(parent) = parent {
            self.nodes
                .get_mut(&parent)
                .expect("validated parent")
                .children
                .push(id);
        } else {
            self.roots.insert(id);
        }
        self.repaint_requested = true;
        Ok(id)
    }

    pub fn node(&self, id: u64) -> Result<&Node, &'static str> {
        self.nodes.get(&id).ok_or("GUI_RESOURCE_CLOSED")
    }

    pub fn node_mut(&mut self, id: u64) -> Result<&mut Node, &'static str> {
        self.repaint_requested = true;
        self.nodes.get_mut(&id).ok_or("GUI_RESOURCE_CLOSED")
    }

    pub fn bind_event(
        &mut self,
        id: u64,
        event: String,
        callback: u64,
    ) -> Result<Option<u64>, &'static str> {
        let node = self.node_mut(id)?;
        if !node.events.contains_key(&event) && node.events.len() >= MAX_EVENTS_PER_NODE {
            return Err("GUI_EVENT_LIMIT");
        }
        Ok(node.events.insert(event, callback))
    }

    pub fn remove(&mut self, id: u64) -> Vec<u64> {
        let Some(children) = self.nodes.get(&id).map(|node| node.children.clone()) else {
            return Vec::new();
        };
        let mut callbacks = Vec::new();
        for child in children {
            callbacks.extend(self.remove(child));
        }
        let node = self
            .nodes
            .remove(&id)
            .expect("node existed before child removal");
        if let Some(parent) = node.parent {
            if let Some(parent) = self.nodes.get_mut(&parent) {
                parent.children.retain(|child| *child != id);
            }
        } else {
            self.roots.remove(&id);
        }
        callbacks.extend(node.events.into_values());
        match node.kind {
            NodeKind::Timer(timer) => callbacks.extend(timer.callback),
            NodeKind::Application { .. } => self.exit_requested = true,
            _ => {}
        }
        self.repaint_requested = true;
        callbacks
    }

    /// Atomically tears down the backend model after the native event loop exits.
    ///
    /// The ABI resource objects may outlive the event loop until the Yanxu VM is
    /// dropped.  Draining here makes application exit observable as a zero-live-
    /// resource state while keeping their later destructors idempotent.
    pub fn clear(&mut self) -> Vec<u64> {
        let mut callbacks = Vec::new();
        for (_, node) in std::mem::take(&mut self.nodes) {
            callbacks.extend(node.events.into_values());
            if let NodeKind::Timer(timer) = node.kind {
                callbacks.extend(timer.callback);
            }
        }
        self.roots.clear();
        self.exit_requested = true;
        self.repaint_requested = true;
        callbacks
    }

    pub fn due_timers(&mut self, now: Instant) -> Vec<(u64, u64)> {
        let mut due = Vec::new();
        for node in self.nodes.values_mut() {
            let NodeKind::Timer(timer) = &mut node.kind else {
                continue;
            };
            if timer.cancelled || timer.next > now {
                continue;
            }
            let Some(callback) = timer.callback else {
                continue;
            };
            due.push((node.id, callback));
            if timer.repeating {
                while timer.next <= now {
                    timer.next += timer.interval;
                }
            } else {
                timer.cancelled = true;
            }
        }
        due
    }

    pub fn counts(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for node in self.nodes.values() {
            let name = match node.kind {
                NodeKind::Application { .. } => "应用",
                NodeKind::Window(_) => "窗口",
                NodeKind::Layout(_) => "布局",
                NodeKind::Control(_) => "控件",
                NodeKind::Timer(_) => "定时器",
            };
            *counts.entry(name.into()).or_insert(0) += 1;
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_tracks_parentage_callbacks_and_timer_lifecycle() {
        let mut model = Model::default();
        let app = model
            .create(
                None,
                NodeKind::Application {
                    title: "测试".into(),
                    theme: "系统".into(),
                },
            )
            .unwrap();
        let window = model
            .create(Some(app), NodeKind::Window(WindowState::default()))
            .unwrap();
        let button = model
            .create(
                Some(window),
                NodeKind::Control(ControlState::new(ControlKind::Button)),
            )
            .unwrap();
        assert_eq!(model.bind_event(button, "点击".into(), 7).unwrap(), None);
        assert_eq!(model.counts()["控件"], 1);
        assert_eq!(model.remove(button), vec![7]);
        assert!(model.node(button).is_err());
    }

    #[test]
    fn clear_releases_every_retained_callback_and_empties_the_model() {
        let mut model = Model::default();
        let app = model
            .create(
                None,
                NodeKind::Application {
                    title: "测试".into(),
                    theme: "系统".into(),
                },
            )
            .unwrap();
        let window = model
            .create(Some(app), NodeKind::Window(WindowState::default()))
            .unwrap();
        let button = model
            .create(
                Some(window),
                NodeKind::Control(ControlState::new(ControlKind::Button)),
            )
            .unwrap();
        model.bind_event(button, "点击".into(), 10).unwrap();
        model
            .create(
                Some(app),
                NodeKind::Timer(TimerState {
                    callback: Some(11),
                    interval: Duration::from_millis(5),
                    next: Instant::now(),
                    repeating: true,
                    cancelled: false,
                }),
            )
            .unwrap();

        let mut callbacks = model.clear();
        callbacks.sort_unstable();
        assert_eq!(callbacks, vec![10, 11]);
        assert!(model.nodes.is_empty());
        assert!(model.roots.is_empty());
        assert!(model.exit_requested);
    }

    #[test]
    fn removing_a_parent_closes_the_whole_subtree_child_first() {
        let mut model = Model::default();
        let app = model
            .create(
                None,
                NodeKind::Application {
                    title: "测试".into(),
                    theme: "系统".into(),
                },
            )
            .unwrap();
        let window = model
            .create(Some(app), NodeKind::Window(WindowState::default()))
            .unwrap();
        let layout = model
            .create(
                Some(window),
                NodeKind::Layout(LayoutState::new(LayoutKind::Vertical)),
            )
            .unwrap();
        let button = model
            .create(
                Some(layout),
                NodeKind::Control(ControlState::new(ControlKind::Button)),
            )
            .unwrap();
        model.bind_event(button, "点击".into(), 21).unwrap();
        model.bind_event(window, "关闭".into(), 22).unwrap();

        assert_eq!(model.remove(window), vec![21, 22]);
        assert!(model.node(window).is_err());
        assert!(model.node(layout).is_err());
        assert!(model.node(button).is_err());
        assert!(model.node(app).is_ok());
        assert!(model.node(app).unwrap().children.is_empty());
        assert!(model.remove(window).is_empty());
    }

    #[test]
    fn event_bindings_are_bounded_but_existing_names_can_be_replaced() {
        let mut model = Model::default();
        let app = model
            .create(
                None,
                NodeKind::Application {
                    title: "测试".into(),
                    theme: "系统".into(),
                },
            )
            .expect("create application");
        for index in 0..MAX_EVENTS_PER_NODE {
            assert_eq!(
                model.bind_event(app, format!("事件{index}"), index as u64),
                Ok(None)
            );
        }
        assert_eq!(
            model.bind_event(app, "超额事件".into(), 999),
            Err("GUI_EVENT_LIMIT")
        );
        assert_eq!(model.bind_event(app, "事件0".into(), 1000), Ok(Some(0)));
    }

    #[test]
    fn model_node_count_and_tree_depth_have_hard_limits() {
        let mut deep = Model::default();
        let mut parent = deep
            .create(
                None,
                NodeKind::Application {
                    title: "测试".into(),
                    theme: "系统".into(),
                },
            )
            .expect("create root");
        for _ in 1..MAX_MODEL_DEPTH {
            parent = deep
                .create(
                    Some(parent),
                    NodeKind::Layout(LayoutState::new(LayoutKind::Vertical)),
                )
                .expect("depth within limit");
        }
        assert_eq!(
            deep.create(
                Some(parent),
                NodeKind::Layout(LayoutState::new(LayoutKind::Vertical))
            ),
            Err("GUI_RESOURCE_LIMIT")
        );

        let mut wide = Model::default();
        for _ in 0..MAX_MODEL_NODES {
            wide.create(
                None,
                NodeKind::Application {
                    title: String::new(),
                    theme: String::new(),
                },
            )
            .expect("node count within limit");
        }
        assert_eq!(
            wide.create(
                None,
                NodeKind::Application {
                    title: String::new(),
                    theme: String::new(),
                }
            ),
            Err("GUI_RESOURCE_LIMIT")
        );
    }
}

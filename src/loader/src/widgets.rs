//! The Widget Inspector: the open interfaces as a tree, the selected widget's fields and its bounds drawn on the client.

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use injector::{TreeWidget, WidgetDetail, WidgetTree};

/// The game value kind that names interfaces (`ipc::state::game_val::INTERFACE`, which the loader does not link); the id is the group.
pub const INTERFACE_GAME_VAL: u32 = 14;
/// How often "Follow mouse" looks at where the client's mouse is.
pub const FOLLOW_EVERY: Duration = Duration::from_millis(100);
/// RuneLite's colour for the selected widget (cyan).
pub const SELECTED_COLOR: &str = "00ffff";

/// One read of the client: the tree, and the names of the interface groups it hasn't named yet.
pub struct Poll {
    pub tree: WidgetTree,
    pub names: Vec<(i32, String)>,
}

/// How a node hangs in the tree: RuneLite's letters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// a top of the root interface
    Root,
    /// a child a script made (dynamic)
    Dynamic,
    /// a component of an interface
    Static,
    /// the top of an interface opened inside a component
    Nested,
}

impl Kind {
    pub fn letter(self) -> char {
        match self {
            Kind::Root => 'R',
            Kind::Dynamic => 'D',
            Kind::Static => 'S',
            Kind::Nested => 'N',
        }
    }
}

pub struct Node {
    pub widget: injector::Widget,
    pub detail: WidgetDetail,
    pub kind: Kind,
    pub children: Vec<usize>,
}

/// The widgets of one read, linked up.
#[derive(Default)]
pub struct Built {
    pub nodes: Vec<Node>,
    pub roots: Vec<usize>,
}

impl Built {
    /// Links the widgets: dynamic children, then static children, then nested interfaces, under each node.
    pub fn new(tree: &WidgetTree) -> Self {
        let mut nodes: Vec<Node> = Vec::with_capacity(tree.widgets.len());
        let mut by_id: HashMap<i32, usize> = HashMap::new();
        for TreeWidget { widget, detail, dynamic, .. } in &tree.widgets {
            if !dynamic {
                by_id.insert(widget.id, nodes.len());
            }
            nodes.push(Node { widget: widget.clone(), detail: detail.clone(), kind: Kind::Static, children: Vec::new() });
        }
        let opened_in: HashMap<i32, i32> = tree.sub_interfaces.iter().map(|sub| (sub.group, sub.parent)).collect();
        let mut roots = Vec::new();
        // (parent node, order within it, child): sorted afterwards, so a node's children come out dynamic, static, nested
        let mut links: Vec<(usize, u8, usize)> = Vec::new();
        for (index, TreeWidget { widget, dynamic, owner, .. }) in tree.widgets.iter().enumerate() {
            let (parent, kind) = if *dynamic {
                (by_id.get(owner).copied(), Kind::Dynamic)
            } else if widget.parent != -1 {
                (by_id.get(&widget.parent).copied(), Kind::Static)
            } else if widget.group == tree.root {
                (None, Kind::Root)
            } else {
                (opened_in.get(&widget.group).and_then(|mount| by_id.get(mount)).copied(), Kind::Nested)
            };
            nodes[index].kind = if parent.is_none() { Kind::Root } else { kind };
            match parent {
                Some(parent) => links.push((parent, kind_order(kind), index)),
                None => roots.push(index),
            }
        }
        links.sort_by_key(|&(parent, order, child)| (parent, order, child));
        for (parent, _, child) in links {
            nodes[parent].children.push(child);
        }
        Self { nodes, roots }
    }

    /// The node for a widget, by component id and index (a dynamic child has its own index under its owner's id).
    pub fn find(&self, id: i32, index: i32) -> Option<usize> {
        self.nodes.iter().position(|node| node.widget.id == id && node.widget.index == index)
    }

    /// The indices from a root down to `target`, for opening the tree to it.
    pub fn path_to(&self, target: usize) -> Vec<usize> {
        let mut parent_of = HashMap::new();
        for (index, node) in self.nodes.iter().enumerate() {
            for &child in &node.children {
                parent_of.insert(child, index);
            }
        }
        let mut path = vec![target];
        while let Some(&parent) = parent_of.get(path.last().unwrap()) {
            path.push(parent);
        }
        path.reverse();
        path
    }

    /// The most specific widget under a canvas point: the smallest shown area, the later in tree order on a tie.
    pub fn under(&self, x: i32, y: i32, hide_hidden: bool) -> Option<usize> {
        let mut found: Option<(i64, usize)> = None;
        let mut stack: Vec<usize> = self.roots.iter().rev().copied().collect();
        while let Some(index) = stack.pop() {
            let widget = &self.nodes[index].widget;
            if hide_hidden && widget.hidden {
                continue;
            }
            let area = i64::from(widget.width) * i64::from(widget.height);
            let inside = (widget.x..widget.x + widget.width).contains(&x) && (widget.y..widget.y + widget.height).contains(&y);
            if !widget.hidden && area > 0 && inside && found.is_none_or(|(smallest, _)| area <= smallest) {
                found = Some((area, index));
            }
            stack.extend(self.nodes[index].children.iter().rev());
        }
        found.map(|(_, index)| index)
    }
}

fn kind_order(kind: Kind) -> u8 {
    match kind {
        Kind::Dynamic => 0,
        Kind::Static | Kind::Root => 1,
        Kind::Nested => 2,
    }
}

/// RuneLite's names for the widget types.
pub fn type_name(kind: i32) -> &'static str {
    match kind {
        0 => "LAYER",
        2 => "INVENTORY",
        3 => "RECTANGLE",
        4 => "TEXT",
        5 => "GRAPHIC",
        6 => "MODEL",
        7 => "TEXT_INVENTORY",
        8 => "IF1_TOOLTIP",
        9 => "LINE",
        _ => "?",
    }
}

#[derive(Default)]
pub struct Inspector {
    pub hide_hidden: bool,
    pub follow_mouse: bool,
    pub error: Option<String>,
    /// The widget picked in the tree: its component id and index.
    pub selected: Option<(i32, i32)>,
    /// Open the tree to the selection on the next frame (a pick made outside the tree).
    pub reveal: bool,
    built: Built,
    read: bool,
    names: HashMap<i32, String>,
    pid: Option<u32>,
    reading: bool,
    mouse_busy: bool,
    next_follow: Option<Instant>,
}

impl Inspector {
    pub fn new() -> Self {
        Self { hide_hidden: true, ..Self::default() }
    }

    pub fn built(&self) -> &Built {
        &self.built
    }

    /// Whether a read has been made (the tree is empty before one).
    pub fn has_read(&self) -> bool {
        self.read
    }

    /// Whether the inspector is showing client `pid`.
    pub fn has_pid(&self, pid: u32) -> bool {
        self.pid == Some(pid)
    }

    pub fn reading(&self) -> bool {
        self.reading
    }

    /// The interface groups the loader has the name of.
    pub fn known_groups(&self) -> Vec<i32> {
        self.names.keys().copied().collect()
    }

    /// `149.0 inventory` or just `149.0`; `[n]` after it for a child with an index.
    pub fn identifier(&self, widget: &injector::Widget) -> String {
        let mut text = format!("{}.{}", widget.group, widget.child);
        if widget.index != -1 {
            text.push_str(&format!("[{}]", widget.index));
        }
        if let Some(name) = self.names.get(&widget.group) {
            text.push(' ');
            text.push_str(name);
        }
        text
    }

    pub fn start(&mut self, pid: u32) {
        if self.pid != Some(pid) {
            self.pid = Some(pid);
            self.built = Built::default();
            self.read = false;
            self.selected = None;
        }
        self.reading = true;
    }

    pub fn finish(&mut self, pid: u32, result: Result<Poll, String>) {
        self.reading = false;
        if self.pid != Some(pid) {
            return;
        }
        match result {
            Ok(Poll { tree, names }) => {
                self.error = None;
                self.names.extend(names);
                self.built = Built::new(&tree);
                self.read = true;
                // the selection stays if the widget is still there
                if self.selected.is_some_and(|(id, index)| self.built.find(id, index).is_none()) {
                    self.selected = None;
                }
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn selected_node(&self) -> Option<&Node> {
        let (id, index) = self.selected?;
        self.built.find(id, index).map(|node| &self.built.nodes[node])
    }

    /// Selects the widget under the client's mouse, when "Follow mouse" is on and it is time to look.
    pub fn follow_due(&mut self, now: Instant) -> bool {
        if !self.follow_mouse || !self.read || self.mouse_busy || self.next_follow.is_some_and(|at| now < at) {
            return false;
        }
        self.next_follow = Some(now + FOLLOW_EVERY);
        self.mouse_busy = true;
        true
    }

    /// The read of the mouse that `follow_due` asked for is over.
    pub fn mouse_done(&mut self) {
        self.mouse_busy = false;
    }

    /// Picks the widget under a canvas point.
    pub fn pick_at(&mut self, x: i32, y: i32) {
        if let Some(index) = self.built.under(x, y, self.hide_hidden) {
            let widget = &self.built.nodes[index].widget;
            let key = (widget.id, widget.index);
            if self.selected != Some(key) {
                self.selected = Some(key);
                self.reveal = true;
            }
        }
    }

    /// The nodes to open so the selection shows.
    pub fn open_path(&self) -> HashSet<usize> {
        let Some((id, index)) = self.selected else { return HashSet::new() };
        self.built.find(id, index).map(|node| self.built.path_to(node).into_iter().collect()).unwrap_or_default()
    }

    /// The layer that draws the selected widget's bounds on the client (see `injector::scene_tiles`).
    pub fn highlight_layers(&self) -> Vec<String> {
        self.selected_node()
            .filter(|node| node.widget.width > 0 && node.widget.height > 0)
            .map(|node| format!("widget:{},{},{},{},{SELECTED_COLOR}", node.widget.x, node.widget.y, node.widget.width, node.widget.height))
            .into_iter()
            .collect()
    }

    /// The fields of a widget the loader can read, as (name, value) rows: RuneLite's, in its order, for those the agent reads.
    pub fn fields(&self, node: &Node) -> Vec<(&'static str, String)> {
        let (w, d) = (&node.widget, &node.detail);
        let color = |value: i32| if value < 0 { "none".to_owned() } else { format!("{value} (#{:06x})", value & 0xffffff) };
        let id_text = |id: i32| if id == -1 { "-1".to_owned() } else { format!("{id} ({}.{})", id >> 16, id & 0xffff) };
        vec![
            ("Id", format!("{} ({})", w.id, self.identifier(w))),
            ("Node", node.kind.letter().to_string()),
            ("Index", w.index.to_string()),
            ("Type", format!("{} ({})", w.kind, type_name(w.kind))),
            ("ContentType", d.content_type.to_string()),
            ("ParentId", id_text(w.parent)),
            ("SelfHidden", d.self_hidden.to_string()),
            ("Hidden", w.hidden.to_string()),
            ("Text", w.text.clone().unwrap_or_default()),
            ("TextColor", color(w.text_color)),
            ("Opacity", d.opacity.to_string()),
            ("FontId", d.font_id.to_string()),
            ("TextShadowed", d.text_shadowed.to_string()),
            ("ItemId", d.item_id.to_string()),
            ("ItemQuantity", d.item_quantity.to_string()),
            ("SpriteId", w.sprite.to_string()),
            ("SpriteTiling", d.sprite_tiling.to_string()),
            ("Filled", d.filled.to_string()),
            ("OriginalX", d.original_x.to_string()),
            ("OriginalY", d.original_y.to_string()),
            ("OriginalWidth", d.original_width.to_string()),
            ("OriginalHeight", d.original_height.to_string()),
            ("XPositionMode", d.x_position_mode.to_string()),
            ("YPositionMode", d.y_position_mode.to_string()),
            ("WidthMode", d.width_mode.to_string()),
            ("HeightMode", d.height_mode.to_string()),
            ("LineHeight", d.line_height.to_string()),
            ("XTextAlignment", d.x_text_alignment.to_string()),
            ("YTextAlignment", d.y_text_alignment.to_string()),
            ("RelativeX", d.relative_x.to_string()),
            ("RelativeY", d.relative_y.to_string()),
            ("Width", w.width.to_string()),
            ("Height", w.height.to_string()),
            ("CanvasLocation", format!("{}, {}", w.x, w.y)),
            ("Bounds", format!("{}, {}, {}, {}", w.x, w.y, w.width, w.height)),
            ("ScrollX", w.scroll_x.to_string()),
            ("ScrollY", w.scroll_y.to_string()),
            ("ScrollWidth", w.scroll_width.to_string()),
            ("ScrollHeight", w.scroll_height.to_string()),
            ("NoClickThrough", d.no_click_through.to_string()),
            ("Children", node.children.len().to_string()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use injector::{SubInterface, Widget};

    fn widget(group: i32, child: i32, parent: i32, index: i32) -> Widget {
        Widget {
            id: group << 16 | child,
            group,
            child,
            kind: 0,
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            parent,
            sprite: -1,
            index,
            hidden: false,
            text: None,
            text_color: -1,
            scroll_x: 0,
            scroll_y: 0,
            scroll_width: 0,
            scroll_height: 0,
        }
    }

    fn at(mut w: Widget, x: i32, y: i32, width: i32, height: i32) -> Widget {
        (w.x, w.y, w.width, w.height) = (x, y, width, height);
        w
    }

    fn tree() -> WidgetTree {
        let id = |group: i32, child: i32| group << 16 | child;
        WidgetTree {
            root: 161,
            sub_interfaces: vec![SubInterface { group: 149, parent: id(161, 2), parent_group: 161, parent_child: 2 }],
            widgets: vec![
                TreeWidget { widget: at(widget(161, 0, -1, -1), 0, 0, 500, 400), detail: WidgetDetail::default(), dynamic: false, owner: -1 },
                TreeWidget { widget: widget(161, 2, id(161, 0), -1), detail: WidgetDetail::default(), dynamic: false, owner: -1 },
                TreeWidget { widget: at(widget(149, 0, -1, -1), 20, 20, 100, 100), detail: WidgetDetail::default(), dynamic: false, owner: -1 },
                TreeWidget { widget: at(widget(149, 1, id(149, 0), -1), 30, 30, 20, 20), detail: WidgetDetail::default(), dynamic: false, owner: -1 },
                TreeWidget { widget: at(widget(149, 0, id(149, 0), 0), 40, 40, 5, 5), detail: WidgetDetail::default(), dynamic: true, owner: id(149, 0) },
            ],
        }
    }

    #[test]
    fn links_dynamic_then_static_then_nested() {
        let built = Built::new(&tree());
        assert_eq!(built.roots, [0]);
        // 161.0 has the static 161.2, which has the nested interface 149 as its one child
        assert_eq!(built.nodes[0].children, [1]);
        assert_eq!(built.nodes[1].children, [2]);
        assert_eq!(built.nodes[2].kind, Kind::Nested);
        // under 149.0 the dynamic child comes before the static one
        assert_eq!(built.nodes[2].children, [4, 3]);
        assert_eq!(built.nodes[4].kind, Kind::Dynamic);
        assert_eq!(built.nodes[3].kind, Kind::Static);
        assert_eq!(built.path_to(3), [0, 1, 2, 3]);
    }

    #[test]
    fn a_widget_with_nowhere_to_hang_is_a_root() {
        let mut tree = tree();
        tree.sub_interfaces.clear();
        let built = Built::new(&tree);
        assert_eq!(built.roots, [0, 2]);
        assert_eq!(built.nodes[2].kind, Kind::Root);
    }

    #[test]
    fn picks_the_smallest_widget_under_a_point() {
        let built = Built::new(&tree());
        // inside the dynamic child (40, 40, 5, 5), which is inside the static one
        assert_eq!(built.under(41, 41, true), Some(4));
        assert_eq!(built.under(31, 31, true), Some(3));
        assert_eq!(built.under(100, 100, true), Some(2));
        assert_eq!(built.under(400, 300, true), Some(0));
        assert_eq!(built.under(600, 600, true), None);
    }

    #[test]
    fn hidden_widgets_are_not_picked() {
        let mut tree = tree();
        tree.widgets[3].widget.hidden = true;
        let built = Built::new(&tree);
        assert_eq!(built.under(31, 31, true), Some(2));
        assert_eq!(built.under(31, 31, false), Some(2));
    }

    #[test]
    fn the_selection_survives_a_refresh_and_draws_a_box() {
        let mut inspector = Inspector::new();
        inspector.start(1);
        inspector.finish(1, Ok(Poll { tree: tree(), names: vec![(149, "inventory".into())] }));
        inspector.pick_at(31, 31);
        assert_eq!(inspector.selected, Some((149 << 16 | 1, -1)));
        assert!(inspector.reveal);
        assert_eq!(inspector.highlight_layers(), ["widget:30,30,20,20,00ffff"]);
        assert_eq!(inspector.identifier(&inspector.built().nodes[3].widget), "149.1 inventory");
        assert_eq!(inspector.identifier(&inspector.built().nodes[4].widget), "149.0[0] inventory");
        inspector.start(1);
        inspector.finish(1, Ok(Poll { tree: tree(), names: vec![] }));
        assert_eq!(inspector.selected, Some((149 << 16 | 1, -1)));
        // a widget that went away is no longer selected
        let mut gone = tree();
        gone.widgets.remove(3);
        inspector.start(1);
        inspector.finish(1, Ok(Poll { tree: gone, names: vec![] }));
        assert_eq!(inspector.selected, None);
        assert!(inspector.highlight_layers().is_empty());
    }
}

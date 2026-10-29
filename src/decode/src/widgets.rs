use serde::{Deserialize, Serialize};
use ipc::state::{InterfaceLayout, interfaces::*};

pub use crate::memory::Memory;

const MAX_DEPTH: usize = 64;
const MAX_BUCKETS: u32 = 1 << 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Widget {
    pub id: i32,
    pub group: i32,
    pub child: i32,
    pub kind: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub parent: i32,
    pub sprite: i32,
    #[serde(default = "minus_one")]
    pub index: i32,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default = "minus_one")]
    pub text_color: i32,
    #[serde(default)]
    pub scroll_x: i32,
    #[serde(default)]
    pub scroll_y: i32,
    #[serde(default)]
    pub scroll_width: i32,
    #[serde(default)]
    pub scroll_height: i32,
}

fn minus_one() -> i32 {
    -1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubInterface {
    pub group: i32,
    pub parent: i32,
    pub parent_group: i32,
    pub parent_child: i32,
}

impl Widget {
    pub fn scaled(self, scale: f32) -> Self {
        let at = |v: i32| (v as f32 * scale).floor() as i32;
        let (x, y) = (at(self.x), at(self.y));
        Self {
            x,
            y,
            width: at(self.x + self.width) - x,
            height: at(self.y + self.height) - y,
            scroll_x: at(self.scroll_x),
            scroll_y: at(self.scroll_y),
            scroll_width: at(self.scroll_width),
            scroll_height: at(self.scroll_height),
            ..self
        }
    }
}

struct Tree<'a, M: Memory> {
    memory: &'a M,
    layout: &'a InterfaceLayout,
    client: u64,
}

impl<M: Memory> Tree<'_, M> {
    fn field(&self, component: u64, offset: u32) -> Option<i32> {
        self.memory.i32(component + offset as u64)
    }

    fn component(&self, id: i32) -> Option<u64> {
        ipc::read::component(self.memory, self.layout, self.client, id)
    }

    fn opened_in(&self, group: i32) -> Option<i32> {
        self.sub_interfaces()?.into_iter().find(|&(_, g)| g == group).map(|(parent, _)| parent)
    }

    fn sub_interfaces(&self) -> Option<Vec<(i32, i32)>> {
        let table = self.client + self.layout.sub_interfaces as u64;
        let buckets = self.memory.ptr(table + TABLE_BUCKETS as u64)?;
        let count = self.memory.i32(table + TABLE_BUCKET_COUNT as u64)? as u32;
        if count == 0 || count > MAX_BUCKETS {
            return None;
        }
        let end = self.memory.u64(buckets + count as u64 * 8)?;
        let mut found = Vec::new();
        for bucket in 0..count as u64 {
            let mut node = self.memory.u64(buckets + bucket * 8)?;
            for _ in 0..MAX_DEPTH {
                if node == 0 || node == end {
                    break;
                }
                found.push((self.memory.i32(node + NODE_KEY as u64)?, self.memory.i32(node + NODE_GROUP as u64)?));
                node = self.memory.u64(node + NODE_NEXT as u64)?;
            }
        }
        Some(found)
    }
}

pub fn sub_interfaces(memory: &impl Memory, layout: &InterfaceLayout) -> Option<Vec<SubInterface>> {
    let client = memory.ptr(layout.module_base + layout.client as u64)?;
    let tree = Tree { memory, layout, client };
    let mut list: Vec<SubInterface> = tree
        .sub_interfaces()?
        .into_iter()
        .map(|(parent, group)| SubInterface { group, parent, parent_group: parent >> 16, parent_child: parent & 0xffff })
        .collect();
    list.sort_by_key(|s| (s.parent_group, s.parent_child));
    Some(list)
}

fn place<M: Memory>(tree: &Tree<M>, component: u64, root: i32) -> Option<(i32, i32, bool)> {
    let layout = tree.layout;
    let field = |c: u64, offset: u32| tree.field(c, offset);
    let hidden = |c: u64| -> Option<bool> {
        if layout.hidden == 0 {
            return Some(false);
        }
        let mut b = [0u8];
        tree.memory.read(c + layout.hidden as u64, &mut b).then_some(b[0] != 0)
    };
    let (mut x, mut y) = (field(component, layout.x)?, field(component, layout.x + 4)?);
    let mut is_hidden = hidden(component)?;
    let mut current = component;
    for _ in 0..MAX_DEPTH {
        let mut parent = field(current, layout.parent)?;
        if parent == -1 {
            let current_group = field(current, layout.id)? >> 16;
            if current_group == root {
                return Some((x, y, is_hidden));
            }
            parent = tree.opened_in(current_group)?;
        }
        current = tree.component(parent)?;
        x += field(current, layout.x)? - field(current, layout.scroll_x)?;
        y += field(current, layout.x + 4)? - field(current, layout.scroll_x + 4)?;
        is_hidden |= hidden(current)?;
    }
    None
}

fn widget_at<M: Memory>(tree: &Tree<M>, component: u64, (x, y, hidden): (i32, i32, bool)) -> Option<Widget> {
    let layout = tree.layout;
    let field = |offset: u32| tree.field(component, offset);
    let optional = |offset: u32| if offset == 0 { Some(-1) } else { field(offset) };
    let id = field(layout.id)?;
    let kind = field(layout.kind)?;
    let text = (layout.text != 0)
        .then(|| crate::memory::read_string(tree.memory, component + layout.text as u64))
        .flatten()
        .filter(|text| !text.is_empty() || kind == TEXT);
    let scroll = |offset: u32, extra: u32| if offset == 0 { Some(0) } else { field(offset + extra) };
    Some(Widget {
        id,
        group: id >> 16,
        child: id & 0xffff,
        kind,
        x,
        y,
        width: field(layout.width)?,
        height: field(layout.width + 4)?,
        parent: field(layout.parent)?,
        sprite: optional(layout.sprite)?,
        index: if layout.index == 0 { -1 } else { field(layout.index)? },
        hidden,
        text,
        text_color: optional(layout.text_color)?,
        scroll_x: field(layout.scroll_x)?,
        scroll_y: field(layout.scroll_x + 4)?,
        scroll_width: scroll(layout.scroll_width, 0)?,
        scroll_height: scroll(layout.scroll_width, 4)?,
    })
}

const TEXT: i32 = 4;
const MAX_CHILDREN: u64 = 1 << 14;

pub fn find_widget(memory: &impl Memory, layout: &InterfaceLayout, root: i32, group: u16, child: u16) -> Option<Widget> {
    if root < 0 {
        return None;
    }
    let client = memory.ptr(layout.module_base + layout.client as u64)?;
    let tree = Tree { memory, layout, client };
    let component = tree.component((group as i32) << 16 | child as i32)?;
    widget_at(&tree, component, place(&tree, component, root)?)
}

pub fn find_children(memory: &impl Memory, layout: &InterfaceLayout, root: i32, group: u16, child: u16) -> Option<Vec<Widget>> {
    if root < 0 || layout.children == 0 {
        return None;
    }
    let client = memory.ptr(layout.module_base + layout.client as u64)?;
    let tree = Tree { memory, layout, client };
    let parent = tree.component((group as i32) << 16 | child as i32)?;
    let placed = place(&tree, parent, root)?;
    dynamic_children(&tree, parent, placed)
}

// The dynamic children of `parent`, which stands at `placed` (see `place`).
fn dynamic_children<M: Memory>(tree: &Tree<M>, parent: u64, placed: (i32, i32, bool)) -> Option<Vec<Widget>> {
    Some(dynamic_children_with_components(tree, parent, placed)?.into_iter().map(|(widget, _)| widget).collect())
}

// The same with the address of each child's component.
fn dynamic_children_with_components<M: Memory>(tree: &Tree<M>, parent: u64, (x, y, hidden): (i32, i32, bool)) -> Option<Vec<(Widget, u64)>> {
    let (memory, layout) = (tree.memory, tree.layout);
    let (x, y) = (x - tree.field(parent, layout.scroll_x)?, y - tree.field(parent, layout.scroll_x + 4)?);
    let count = memory.u64(parent + layout.children as u64)?;
    if count == 0 {
        return Some(Vec::new());
    }
    if count > MAX_CHILDREN {
        return None;
    }
    let entries = memory.ptr(parent + layout.children as u64 + CHILDREN_DATA as u64)?;
    let mut children = Vec::new();
    for i in 0..count {
        let Some(component) = memory.ptr(entries + i * ENTRY_SIZE as u64 + ENTRY_COMPONENT as u64) else { continue };
        let at = (x + tree.field(component, layout.x)?, y + tree.field(component, layout.x + 4)?, hidden);
        let mut widget = widget_at(tree, component, at)?;
        if layout.hidden != 0 {
            let mut b = [0u8];
            widget.hidden |= memory.read(component + layout.hidden as u64, &mut b) && b[0] != 0;
        }
        children.push((widget, component));
    }
    Some(children)
}

/// Component fields read at fixed offsets, found by matching decoded cache definitions against memory (build a2636a01); redo after a client update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetDetail {
    pub content_type: i32,
    /// the widget's own hidden flag; `Widget::hidden` is also set when an ancestor is hidden
    pub self_hidden: bool,
    /// the settings it was laid out from: positions and sizes, and the modes that say how they are used
    pub original_x: i32,
    pub original_y: i32,
    pub original_width: i32,
    pub original_height: i32,
    pub x_position_mode: i32,
    pub y_position_mode: i32,
    pub width_mode: i32,
    pub height_mode: i32,
    /// where it was put in its parent, in client pixels (before the interface scale)
    pub relative_x: i32,
    pub relative_y: i32,
    pub relative_width: i32,
    pub relative_height: i32,
    /// 255 is invisible, 0 opaque
    pub opacity: i32,
    pub font_id: i32,
    pub line_height: i32,
    pub x_text_alignment: i32,
    pub y_text_alignment: i32,
    pub text_shadowed: bool,
    pub filled: bool,
    pub sprite_tiling: bool,
    pub no_click_through: bool,
    pub item_id: i32,
    pub item_quantity: i32,
}

const DETAIL_CONTENT_TYPE: u64 = 0x30;
const DETAIL_X_MODE: u64 = 0x3c;
const DETAIL_ORIGINAL: u64 = 0x4c;
const DETAIL_RELATIVE: u64 = 0x5c;
const DETAIL_FILLED: u64 = 0xa8;
const DETAIL_OPACITY: u64 = 0xb0;
const DETAIL_SPRITE_TILING: u64 = 0xec;
const DETAIL_FONT: u64 = 0x150;
const DETAIL_LINE_HEIGHT: u64 = 0x188;
const DETAIL_X_ALIGNMENT: u64 = 0x18c;
const DETAIL_TEXT_SHADOWED: u64 = 0x194;
const DETAIL_ITEM_ID: u64 = 0xb38;
const DETAIL_NO_CLICK_THROUGH: u64 = 0xb98;

fn detail_at<M: Memory>(tree: &Tree<M>, component: u64) -> WidgetDetail {
    let memory = tree.memory;
    let int = |offset: u64| memory.i32(component + offset).unwrap_or(0);
    let byte = |offset: u64| {
        let mut b = [0u8];
        memory.read(component + offset, &mut b) && b[0] != 0
    };
    let mut opacity = [0u8];
    memory.read(component + DETAIL_OPACITY, &mut opacity);
    WidgetDetail {
        content_type: int(DETAIL_CONTENT_TYPE),
        self_hidden: tree.layout.hidden != 0 && byte(tree.layout.hidden as u64),
        original_x: int(DETAIL_ORIGINAL),
        original_y: int(DETAIL_ORIGINAL + 4),
        original_width: int(DETAIL_ORIGINAL + 8),
        original_height: int(DETAIL_ORIGINAL + 12),
        x_position_mode: int(DETAIL_X_MODE),
        y_position_mode: int(DETAIL_X_MODE + 4),
        width_mode: int(DETAIL_X_MODE + 8),
        height_mode: int(DETAIL_X_MODE + 12),
        relative_x: int(DETAIL_RELATIVE),
        relative_y: int(DETAIL_RELATIVE + 4),
        relative_width: int(DETAIL_RELATIVE + 8),
        relative_height: int(DETAIL_RELATIVE + 12),
        opacity: opacity[0] as i32,
        font_id: int(DETAIL_FONT),
        line_height: int(DETAIL_LINE_HEIGHT),
        x_text_alignment: int(DETAIL_X_ALIGNMENT),
        y_text_alignment: int(DETAIL_X_ALIGNMENT + 4),
        text_shadowed: byte(DETAIL_TEXT_SHADOWED),
        filled: byte(DETAIL_FILLED),
        sprite_tiling: byte(DETAIL_SPRITE_TILING),
        no_click_through: byte(DETAIL_NO_CLICK_THROUGH),
        item_id: int(DETAIL_ITEM_ID),
        item_quantity: int(DETAIL_ITEM_ID + 4),
    }
}

/// A widget of the inspector's tree: a component of an interface (static) or a child a script made for one (dynamic).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeWidget {
    pub widget: Widget,
    pub detail: WidgetDetail,
    /// true for a dynamic child, which hangs off the component with id `owner`
    pub dynamic: bool,
    pub owner: i32,
}

/// Every open interface with all its components and dynamic children, linked by `parent` and `sub_interfaces`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetTree {
    pub root: i32,
    pub sub_interfaces: Vec<SubInterface>,
    pub widgets: Vec<TreeWidget>,
}

/// The most components one group is taken to hold.
const MAX_GROUP: u64 = 1 << 14;

// How many components `group` has.
fn group_size<M: Memory>(tree: &Tree<M>, group: i32) -> Option<u64> {
    let manager = tree.memory.ptr(tree.client + tree.layout.manager as u64)?;
    let groups = tree.memory.ptr(manager + tree.layout.groups as u64)?;
    let count = tree.memory.u64(groups + group as u64 * GROUP_SIZE as u64 + ARRAY_COUNT as u64)?;
    (count <= MAX_GROUP).then_some(count)
}

/// The id, index and address of every component of the open groups, for reverse engineering fields.
pub fn component_addresses(memory: &impl Memory, layout: &InterfaceLayout, root: i32) -> Option<Vec<(i32, i32, u64)>> {
    let client = memory.ptr(layout.module_base + layout.client as u64)?;
    let tree = Tree { memory, layout, client };
    let mut groups = vec![root];
    for sub in sub_interfaces(memory, layout)? {
        if !groups.contains(&sub.group) {
            groups.push(sub.group);
        }
    }
    let mut found = Vec::new();
    for group in groups {
        for child in 0..group_size(&tree, group).unwrap_or(0) {
            let id = group << 16 | child as i32;
            if let Some(component) = tree.component(id) {
                found.push((id, -1, component));
                let count = memory.u64(component + layout.children as u64).filter(|&count| count <= MAX_CHILDREN).unwrap_or(0);
                let entries = if count == 0 { None } else { memory.ptr(component + layout.children as u64 + CHILDREN_DATA as u64) };
                for i in 0..count {
                    if let Some(child) = entries.and_then(|entries| memory.ptr(entries + i * ENTRY_SIZE as u64 + ENTRY_COMPONENT as u64)) {
                        found.push((id, i as i32, child));
                    }
                }
            }
        }
    }
    Some(found)
}

pub fn find_tree(memory: &impl Memory, layout: &InterfaceLayout, root: i32, scale: f32) -> Option<WidgetTree> {
    if root < 0 {
        return None;
    }
    let client = memory.ptr(layout.module_base + layout.client as u64)?;
    let tree = Tree { memory, layout, client };
    let sub_interfaces = sub_interfaces(memory, layout)?;
    let mut groups = vec![root];
    for sub in &sub_interfaces {
        if !groups.contains(&sub.group) {
            groups.push(sub.group);
        }
    }
    let mut widgets = Vec::new();
    for group in groups {
        for child in 0..group_size(&tree, group).unwrap_or(0) {
            let id = group << 16 | child as i32;
            let Some(component) = tree.component(id) else { continue };
            let Some(placed) = place(&tree, component, root) else { continue };
            let Some(widget) = widget_at(&tree, component, placed) else { continue };
            widgets.push(TreeWidget { widget: widget.scaled(scale), detail: detail_at(&tree, component), dynamic: false, owner: -1 });
            for (child, address) in dynamic_children_with_components(&tree, component, placed).unwrap_or_default() {
                widgets.push(TreeWidget { widget: child.scaled(scale), detail: detail_at(&tree, address), dynamic: true, owner: id });
            }
        }
    }
    Some(WidgetTree { root, sub_interfaces, widgets })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Fake;

    #[test]
    fn reads_the_detail_fields_at_their_offsets() {
        let mut m = Fake::default();
        let c = 0x20_0000u64;
        let words = [
            (DETAIL_CONTENT_TYPE, 7),
            (DETAIL_X_MODE, 1),
            (DETAIL_X_MODE + 4, 2),
            (DETAIL_X_MODE + 8, 1),
            (DETAIL_X_MODE + 12, 0),
            (DETAIL_ORIGINAL, 4),
            (DETAIL_ORIGINAL + 4, 16),
            (DETAIL_ORIGINAL + 8, 23),
            (DETAIL_ORIGINAL + 12, 13),
            (DETAIL_RELATIVE, 5),
            (DETAIL_RELATIVE + 12, 14),
            (DETAIL_FONT, 494),
            (DETAIL_LINE_HEIGHT, 3),
            (DETAIL_X_ALIGNMENT, 1),
            (DETAIL_X_ALIGNMENT + 4, 2),
            (DETAIL_ITEM_ID, 1891),
            (DETAIL_ITEM_ID + 4, 28),
        ];
        for (offset, value) in words {
            m.put_i32(c + offset, value);
        }
        for (offset, value) in [(LAYOUT.hidden as u64, 1u8), (DETAIL_OPACITY, 200), (DETAIL_FILLED, 1), (DETAIL_TEXT_SHADOWED, 1), (DETAIL_SPRITE_TILING, 0)] {
            m.put(c + offset, &[value]);
        }
        let tree = Tree { memory: &m, layout: &LAYOUT, client: 0 };
        let d = detail_at(&tree, c);
        assert_eq!((d.content_type, d.self_hidden, d.opacity, d.filled, d.text_shadowed, d.sprite_tiling, d.no_click_through), (7, true, 200, true, true, false, false));
        assert_eq!((d.x_position_mode, d.y_position_mode, d.width_mode, d.height_mode), (1, 2, 1, 0));
        assert_eq!((d.original_x, d.original_y, d.original_width, d.original_height), (4, 16, 23, 13));
        assert_eq!((d.relative_x, d.relative_y, d.relative_width, d.relative_height), (5, 0, 0, 14));
        assert_eq!((d.font_id, d.line_height, d.x_text_alignment, d.y_text_alignment, d.item_id, d.item_quantity), (494, 3, 1, 2, 1891, 28));
    }

    const LAYOUT: InterfaceLayout = InterfaceLayout {
        module_base: 0x1_4000_0000,
        client: 0x100,
        root: 0x10,
        manager: 0x20,
        groups: 0x30,
        sub_interfaces: 0x40,
        id: 0x4,
        kind: 0x28,
        parent: 0x74,
        x: 0x5c,
        width: 0x64,
        scroll_x: 0x84,
        sprite: 0xc8,
        text: 0x158,
        hidden: 0x78,
        text_color: 0x98,
        scroll_width: 0x8c,
        index: 0x8,
        children: 0xb50,
        reserved: 0,
        canvas: 0x90,
        ui_width: 0x6c,
        ui_scale: 0x68,
    };
    const CLIENT: u64 = 0x10_0000;

    fn lobby() -> Fake {
        let mut m = Fake::default();
        m.put_u64(LAYOUT.module_base + LAYOUT.client as u64, CLIENT);
        let manager = 0x20_0000;
        m.put_u64(CLIENT + LAYOUT.manager as u64, manager);
        let groups = 0x30_0000;
        m.put_u64(manager + LAYOUT.groups as u64, groups);

        let mut next = 0x40_0000u64;
        let mut component = |m: &mut Fake, group: u64, child: u64, count: u64, parent: i32, pos: (i32, i32), size: (i32, i32)| {
            let array = groups + group * GROUP_SIZE as u64;
            let elements = 0x1000_0000 + group * 0x1_0000;
            m.put_u64(array + ARRAY_COUNT as u64, count);
            m.put_u64(array + ARRAY_DATA as u64, elements);
            let c = next;
            next += 0x1000;
            m.put_u64(elements + child * ENTRY_SIZE as u64 + ENTRY_COMPONENT as u64, c);
            m.put_i32(c + LAYOUT.id as u64, (group << 16 | child) as i32);
            m.put_i32(c + LAYOUT.kind as u64, 0);
            m.put_i32(c + LAYOUT.parent as u64, parent);
            m.put_i32(c + LAYOUT.x as u64, pos.0);
            m.put_i32(c + LAYOUT.x as u64 + 4, pos.1);
            m.put_i32(c + LAYOUT.width as u64, size.0);
            m.put_i32(c + LAYOUT.width as u64 + 4, size.1);
            m.put_i32(c + LAYOUT.scroll_x as u64, 0);
            m.put_i32(c + LAYOUT.scroll_x as u64 + 4, 0);
            m.put_i32(c + LAYOUT.sprite as u64, -1);
            m.put(c + LAYOUT.hidden as u64, &[0]);
            m.put_i32(c + LAYOUT.index as u64, -1);
            m.put_i32(c + LAYOUT.text_color as u64, 0);
            m.put_i32(c + LAYOUT.scroll_width as u64, 0);
            m.put_i32(c + LAYOUT.scroll_width as u64 + 4, 0);
            m.put_short_string(c + LAYOUT.text as u64, "");
            m.put_u64(c + LAYOUT.children as u64, 0);
        };
        component(&mut m, 165, 0, 43, -1, (0, 0), (1360, 1820));
        component(&mut m, 165, 40, 43, 165 << 16, (0, 0), (1360, 1820));
        component(&mut m, 378, 0, 85, -1, (0, 0), (1360, 1820));
        component(&mut m, 378, 1, 85, 378 << 16, (0, 0), (1360, 1820));
        component(&mut m, 378, 2, 85, 378 << 16 | 1, (297, 10), (765, 493));
        component(&mut m, 378, 57, 85, 378 << 16 | 2, (0, 0), (765, 385));
        component(&mut m, 378, 72, 85, 378 << 16 | 57, (268, 282), (229, 90));

        let table = CLIENT + LAYOUT.sub_interfaces as u64;
        let buckets = 0x50_0000;
        m.put_u64(table + TABLE_BUCKETS as u64, buckets);
        m.put_i32(table + TABLE_BUCKET_COUNT as u64, 1);
        let node = 0x60_0000;
        m.put_u64(buckets, node);
        m.put_u64(buckets + 8, 0x70_0000);
        m.put_i32(node + NODE_KEY as u64, 165 << 16 | 40);
        m.put_i32(node + NODE_GROUP as u64, 378);
        m.put_u64(node + NODE_NEXT as u64, 0);
        m
    }

    #[test]
    fn finds_the_lobby_play_button() {
        let widget = find_widget(&lobby(), &LAYOUT, 165, 378, 72).unwrap();
        assert_eq!((widget.x, widget.y, widget.width, widget.height), (565, 292, 229, 90));
        assert_eq!(widget.id, 378 << 16 | 72);
        assert_eq!(widget.parent, 378 << 16 | 57);
        assert_eq!(widget.sprite, -1);
    }

    #[test]
    fn reads_the_sprite() {
        let mut m = lobby();
        let c72 = m.u64(0x1000_0000 + 378 * 0x1_0000 + 72 * ENTRY_SIZE as u64 + ENTRY_COMPONENT as u64).unwrap();
        m.put_i32(c72 + LAYOUT.sprite as u64, 1218);
        assert_eq!(find_widget(&m, &LAYOUT, 165, 378, 72).unwrap().sprite, 1218);
        let unknown = InterfaceLayout { sprite: 0, ..LAYOUT };
        assert_eq!(find_widget(&m, &unknown, 165, 378, 72).unwrap().sprite, -1);
    }

    #[test]
    fn scroll_moves_children() {
        let mut m = lobby();
        let c57 = m.u64(0x1000_0000 + 378 * 0x1_0000 + 57 * ENTRY_SIZE as u64 + ENTRY_COMPONENT as u64).unwrap();
        m.put_i32(c57 + LAYOUT.scroll_x as u64 + 4, 50);
        assert_eq!(find_widget(&m, &LAYOUT, 165, 378, 72).unwrap().y, 242);
    }

    #[test]
    fn scales_to_canvas_pixels() {
        let widget = Widget {
            id: 0,
            group: 149,
            child: 0,
            kind: 0,
            x: 549,
            y: 750,
            width: 190,
            height: 261,
            parent: -1,
            sprite: -1,
            index: -1,
            hidden: false,
            text: None,
            text_color: -1,
            scroll_x: 0,
            scroll_y: 100,
            scroll_width: 0,
            scroll_height: 5104,
        };
        let scaled = widget.clone().scaled(1402.0 / 807.0);
        assert_eq!((scaled.x, scaled.y, scaled.width, scaled.height), (953, 1302, 330, 454));
        assert_eq!((scaled.scroll_y, scaled.scroll_height), (173, 8867));
        assert_eq!(widget.clone().scaled(1.0), widget);
    }

    fn component_of(m: &Fake, group: u64, child: u64) -> u64 {
        m.u64(0x1000_0000 + group * 0x1_0000 + child * ENTRY_SIZE as u64 + ENTRY_COMPONENT as u64).unwrap()
    }

    fn dynamic_child(m: &mut Fake, at: u64, id: i32, index: i32, pos: (i32, i32), text: &str, hidden: bool) {
        m.put_i32(at + LAYOUT.id as u64, id);
        m.put_i32(at + LAYOUT.index as u64, index);
        m.put_i32(at + LAYOUT.kind as u64, TEXT);
        m.put_i32(at + LAYOUT.parent as u64, id);
        m.put_i32(at + LAYOUT.x as u64, pos.0);
        m.put_i32(at + LAYOUT.x as u64 + 4, pos.1);
        m.put_i32(at + LAYOUT.width as u64, 28);
        m.put_i32(at + LAYOUT.width as u64 + 4, 16);
        m.put_i32(at + LAYOUT.scroll_x as u64, 0);
        m.put_i32(at + LAYOUT.scroll_x as u64 + 4, 0);
        m.put_i32(at + LAYOUT.sprite as u64, -1);
        m.put(at + LAYOUT.hidden as u64, &[hidden as u8]);
        m.put_i32(at + LAYOUT.text_color as u64, 0xf0f000);
        m.put_i32(at + LAYOUT.scroll_width as u64, 0);
        m.put_i32(at + LAYOUT.scroll_width as u64 + 4, 0);
        m.put_short_string(at + LAYOUT.text as u64, text);
    }

    #[test]
    fn places_dynamic_children_inside_their_scrolled_parent() {
        let mut m = lobby();
        let c57 = component_of(&m, 378, 57);
        m.put_i32(c57 + LAYOUT.scroll_x as u64 + 4, 16);
        let entries = 0x80_0000;
        m.put_u64(c57 + LAYOUT.children as u64, 2);
        m.put_u64(c57 + LAYOUT.children as u64 + CHILDREN_DATA as u64, entries);
        m.put_u64(entries + ENTRY_COMPONENT as u64, 0x81_0000);
        m.put_u64(entries + ENTRY_SIZE as u64 + ENTRY_COMPONENT as u64, 0x82_0000);
        let id = 378 << 16 | 57;
        dynamic_child(&mut m, 0x81_0000, id, 0, (16, 0), "301", false);
        dynamic_child(&mut m, 0x82_0000, id, 1, (16, 16), "302", true);

        let children = find_children(&m, &LAYOUT, 165, 378, 57).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!((children[0].x, children[0].y, children[0].index), (297 + 16, 10 - 16, 0));
        assert_eq!((children[1].y, children[1].text.as_deref(), children[1].hidden), (10, Some("302"), true));
        assert_eq!((children[0].text_color, children[0].hidden), (0xf0f000, false));
        assert_eq!(find_children(&m, &LAYOUT, 165, 378, 72), Some(Vec::new()));
    }

    #[test]
    fn hidden_parents_hide_their_components() {
        let mut m = lobby();
        assert!(!find_widget(&m, &LAYOUT, 165, 378, 72).unwrap().hidden);
        m.put(component_of(&m, 378, 2) + LAYOUT.hidden as u64, &[1]);
        assert!(find_widget(&m, &LAYOUT, 165, 378, 72).unwrap().hidden);
    }

    #[test]
    fn lists_sub_interfaces() {
        assert_eq!(
            sub_interfaces(&lobby(), &LAYOUT),
            Some(vec![SubInterface { group: 378, parent: 165 << 16 | 40, parent_group: 165, parent_child: 40 }])
        );
    }

    #[test]
    fn closed_or_missing_widgets_are_none() {
        let m = lobby();
        assert_eq!(find_widget(&m, &LAYOUT, 161, 378, 72), None);
        assert_eq!(find_widget(&m, &LAYOUT, 165, 378, 85), None);
        assert_eq!(find_widget(&m, &LAYOUT, 165, 378, 3), None);
        assert_eq!(find_widget(&m, &LAYOUT, -1, 378, 72), None);
    }
}

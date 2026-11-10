//! Native screen-reader accessibility bridge powered by `accesskit`.

pub use accesskit;

use craftui_core::{NodeId, Rect};
use slotmap::Key;
use std::collections::HashMap;

/// Semantic UI element role mapped to `accesskit::Role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum A11yRole {
    Window,
    #[default]
    Container,
    Label,
    Button,
    TextInput,
    MultilineTextInput,
    CheckBox,
    RadioButton,
    Switch,
    Slider,
    ComboBox,
    TabList,
    Tab,
    Menu,
    MenuItem,
    Tooltip,
    Divider,
    ProgressIndicator,
    Image,
    ScrollView,
    Canvas,
    Dialog,
    List,
    ListItem,
}

impl A11yRole {
    pub fn to_accesskit(self) -> accesskit::Role {
        match self {
            Self::Window => accesskit::Role::Window,
            Self::Container => accesskit::Role::GenericContainer,
            Self::Label => accesskit::Role::Label,
            Self::Button => accesskit::Role::Button,
            Self::TextInput => accesskit::Role::TextInput,
            Self::MultilineTextInput => accesskit::Role::MultilineTextInput,
            Self::CheckBox => accesskit::Role::CheckBox,
            Self::RadioButton => accesskit::Role::RadioButton,
            Self::Switch => accesskit::Role::Switch,
            Self::Slider => accesskit::Role::Slider,
            Self::ComboBox => accesskit::Role::ComboBox,
            Self::TabList => accesskit::Role::TabList,
            Self::Tab => accesskit::Role::Tab,
            Self::Menu => accesskit::Role::Menu,
            Self::MenuItem => accesskit::Role::MenuItem,
            Self::Tooltip => accesskit::Role::Tooltip,
            Self::Divider => accesskit::Role::Splitter,
            Self::ProgressIndicator => accesskit::Role::ProgressIndicator,
            Self::Image => accesskit::Role::Image,
            Self::ScrollView => accesskit::Role::ScrollView,
            Self::Canvas => accesskit::Role::Canvas,
            Self::Dialog => accesskit::Role::Dialog,
            Self::List => accesskit::Role::List,
            Self::ListItem => accesskit::Role::ListItem,
        }
    }
}

/// Accessibility properties attached to a widget node.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct A11yMetadata {
    pub role: A11yRole,
    pub label: Option<String>,
    pub value: Option<String>,
    pub numeric_value: Option<f64>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub checked: Option<bool>,
    pub disabled: bool,
    pub focusable: bool,
}

/// Snapshot node handed to [`A11yBridge`] after layout.
#[derive(Debug, Clone)]
pub struct A11ySnapshotNode {
    pub id: NodeId,
    pub bounds: Rect,
    pub meta: A11yMetadata,
    pub children: Vec<NodeId>,
}

/// High-level action decoded from `accesskit::ActionRequest`.
#[derive(Debug, Clone, PartialEq)]
pub enum A11yAction {
    Click(NodeId),
    Focus(NodeId),
    SetValue(NodeId, String),
    SetNumericValue(NodeId, f64),
    Increment(NodeId),
    Decrement(NodeId),
    ScrollIntoView(NodeId),
}

/// Synchronizes CraftUI widget snapshots with `accesskit::TreeUpdate`.
pub struct A11yBridge {
    id_lookup: HashMap<u64, NodeId>,
    window_title: String,
}

impl Default for A11yBridge {
    fn default() -> Self {
        Self::new("CraftUI")
    }
}

impl A11yBridge {
    pub fn new(window_title: impl Into<String>) -> Self {
        Self {
            id_lookup: HashMap::new(),
            window_title: window_title.into(),
        }
    }

    pub fn set_window_title(&mut self, title: impl Into<String>) {
        self.window_title = title.into();
    }

    #[inline]
    pub fn to_ak_id(node: NodeId) -> accesskit::NodeId {
        accesskit::NodeId(node.data().as_ffi())
    }

    /// Build a complete `accesskit::TreeUpdate` from widget snapshots.
    pub fn build_tree_update(
        &mut self,
        root: NodeId,
        focused: Option<NodeId>,
        nodes: &[A11ySnapshotNode],
    ) -> accesskit::TreeUpdate {
        self.id_lookup.clear();
        let mut ak_nodes = Vec::with_capacity(nodes.len());

        for snap in nodes {
            let raw = snap.id.data().as_ffi();
            self.id_lookup.insert(raw, snap.id);

            let mut ak_node = accesskit::Node::new(if snap.id == root {
                accesskit::Role::Window
            } else {
                snap.meta.role.to_accesskit()
            });

            if snap.id == root {
                ak_node.set_label(self.window_title.clone());
            } else if let Some(ref label) = snap.meta.label {
                ak_node.set_label(label.clone());
            }

            if let Some(ref val) = snap.meta.value {
                ak_node.set_value(val.clone());
            }
            if let Some(num) = snap.meta.numeric_value {
                ak_node.set_numeric_value(num);
            }
            if let Some(min) = snap.meta.min_value {
                ak_node.set_min_numeric_value(min);
            }
            if let Some(max) = snap.meta.max_value {
                ak_node.set_max_numeric_value(max);
            }
            if let Some(checked) = snap.meta.checked {
                ak_node.set_toggled(if checked {
                    accesskit::Toggled::True
                } else {
                    accesskit::Toggled::False
                });
            }
            if snap.meta.disabled {
                ak_node.set_disabled();
            }

            ak_node.set_bounds(accesskit::Rect {
                x0: snap.bounds.x as f64,
                y0: snap.bounds.y as f64,
                x1: snap.bounds.max_x() as f64,
                y1: snap.bounds.max_y() as f64,
            });

            match snap.meta.role {
                A11yRole::Button
                | A11yRole::CheckBox
                | A11yRole::Switch
                | A11yRole::RadioButton
                | A11yRole::Tab
                | A11yRole::MenuItem
                | A11yRole::ComboBox => {
                    ak_node.add_action(accesskit::Action::Click);
                }
                A11yRole::Slider => {
                    ak_node.add_action(accesskit::Action::Increment);
                    ak_node.add_action(accesskit::Action::Decrement);
                    ak_node.add_action(accesskit::Action::SetValue);
                }
                A11yRole::TextInput | A11yRole::MultilineTextInput => {
                    ak_node.add_action(accesskit::Action::SetValue);
                    ak_node.add_action(accesskit::Action::Focus);
                }
                _ => {}
            }
            if snap.meta.focusable {
                ak_node.add_action(accesskit::Action::Focus);
            }

            let children: Vec<accesskit::NodeId> =
                snap.children.iter().copied().map(Self::to_ak_id).collect();
            if !children.is_empty() {
                ak_node.set_children(children);
            }

            ak_nodes.push((Self::to_ak_id(snap.id), ak_node));
        }

        let focus_ak = focused
            .map(Self::to_ak_id)
            .unwrap_or_else(|| Self::to_ak_id(root));

        accesskit::TreeUpdate {
            nodes: ak_nodes,
            tree: Some(accesskit::TreeInfo::new(Self::to_ak_id(root))),
            tree_id: accesskit::TreeId::ROOT,
            focus: focus_ak,
        }
    }

    /// Translate an incoming OS screen-reader action into a CraftUI [`A11yAction`].
    pub fn translate_action(&self, req: &accesskit::ActionRequest) -> Option<A11yAction> {
        let node = *self.id_lookup.get(&req.target_node.0)?;
        match req.action {
            accesskit::Action::Click => Some(A11yAction::Click(node)),
            accesskit::Action::Focus => Some(A11yAction::Focus(node)),
            accesskit::Action::Increment => Some(A11yAction::Increment(node)),
            accesskit::Action::Decrement => Some(A11yAction::Decrement(node)),
            accesskit::Action::ScrollIntoView => Some(A11yAction::ScrollIntoView(node)),
            accesskit::Action::SetValue => match &req.data {
                Some(accesskit::ActionData::Value(s)) => {
                    Some(A11yAction::SetValue(node, s.to_string()))
                }
                Some(accesskit::ActionData::NumericValue(v)) => {
                    Some(A11yAction::SetNumericValue(node, *v))
                }
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    #[test]
    fn tree_update_and_action_routing() {
        let mut nodes: SlotMap<NodeId, ()> = SlotMap::with_key();
        let root = nodes.insert(());
        let btn = nodes.insert(());

        let mut bridge = A11yBridge::new("Test App");
        let update = bridge.build_tree_update(
            root,
            Some(btn),
            &[
                A11ySnapshotNode {
                    id: root,
                    bounds: Rect::new(0.0, 0.0, 400.0, 300.0),
                    meta: A11yMetadata::default(),
                    children: vec![btn],
                },
                A11ySnapshotNode {
                    id: btn,
                    bounds: Rect::new(20.0, 20.0, 120.0, 40.0),
                    meta: A11yMetadata {
                        role: A11yRole::Button,
                        label: Some("Submit".into()),
                        focusable: true,
                        ..Default::default()
                    },
                    children: vec![],
                },
            ],
        );

        assert_eq!(update.nodes.len(), 2);
        assert_eq!(update.focus, A11yBridge::to_ak_id(btn));

        let req = accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: A11yBridge::to_ak_id(btn),
            data: None,
        };
        assert_eq!(bridge.translate_action(&req), Some(A11yAction::Click(btn)));
    }
}

use craftui::prelude::*;

pub mod aot_ui {
    include!(concat!(env!("OUT_DIR"), "/todo_app.rs"));
}

use aot_ui::{TaskItem, TodoScreen};

#[derive(Clone)]
pub struct TodoController {
    pub screen: TodoScreen,
    pub all_tasks: Signal<Vec<TaskItem>>,
    pub next_id: Signal<i32>,
}

impl TodoController {
    pub fn add_task(&self, title: &str) {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return;
        }
        let id = self.next_id.get_untracked();
        self.next_id.set(id + 1);
        self.all_tasks.update(|list| {
            list.push(TaskItem {
                id,
                title: trimmed.to_string(),
                completed: false,
            });
        });
    }

    pub fn toggle_visible_at(&self, visible_idx: usize) {
        let vis = self.screen.visible_tasks.get_untracked();
        let Some(target) = vis.get(visible_idx) else {
            return;
        };
        let target_id = target.id;
        self.all_tasks.update(|list| {
            if let Some(item) = list.iter_mut().find(|t| t.id == target_id) {
                item.completed = !item.completed;
            }
        });
    }

    pub fn delete_visible_at(&self, visible_idx: usize) {
        let vis = self.screen.visible_tasks.get_untracked();
        let Some(target) = vis.get(visible_idx) else {
            return;
        };
        let target_id = target.id;
        self.all_tasks.update(|list| {
            list.retain(|t| t.id != target_id);
        });
    }

    pub fn clear_completed(&self) {
        self.all_tasks.update(|list| {
            list.retain(|t| !t.completed);
        });
    }
}

pub fn mount_todo_app(cx: Scope, tree: &mut WidgetTree) -> (NodeId, TodoController) {
    let screen = TodoScreen::new(cx);
    let initial = vec![
        TaskItem {
            id: 1,
            title: "Design cross-platform reactive signal graph".into(),
            completed: true,
        },
        TaskItem {
            id: 2,
            title: "Build reusable .cui component library".into(),
            completed: false,
        },
        TaskItem {
            id: 3,
            title: "Add automated visual snapshot tests".into(),
            completed: false,
        },
    ];
    let all_tasks = cx.signal(initial);
    let next_id = cx.signal(4i32);

    // Keep filtered list, summary banner, and completion progress synced reactively
    let vis_sig = screen.visible_tasks;
    let sum_sig = screen.summary_text;
    let ratio_sig = screen.completion_ratio;
    let tab_sig = screen.filter_tab;
    let query_sig = screen.search_query;
    cx.effect(move || {
        let tasks = all_tasks.get();
        let tab = tab_sig.get();
        let query = query_sig.get().to_lowercase();

        let filtered: Vec<TaskItem> = tasks
            .iter()
            .filter(|t| {
                let matches_tab = match tab {
                    1 => !t.completed,
                    2 => t.completed,
                    _ => true,
                };
                let matches_query =
                    query.trim().is_empty() || t.title.to_lowercase().contains(query.trim());
                matches_tab && matches_query
            })
            .cloned()
            .collect();

        let total = tasks.len();
        let done = tasks.iter().filter(|t| t.completed).count();
        let ratio = if total == 0 {
            0.0
        } else {
            done as f32 / total as f32
        };

        vis_sig.set(filtered);
        sum_sig.set(format!("{done} of {total} completed"));
        ratio_sig.set(ratio);
    });

    let ctrl = TodoController {
        screen: screen.clone(),
        all_tasks,
        next_id,
    };

    let c_add = ctrl.clone();
    screen.set_on_add_task(move || {
        let title = c_add.screen.draft_title.get_untracked();
        if !title.trim().is_empty() {
            c_add.add_task(&title);
            c_add.screen.draft_title.set(String::new());
        }
    });
    let c_tog = ctrl.clone();
    screen.set_on_toggle_index(move |idx| {
        if idx >= 0 {
            c_tog.toggle_visible_at(idx as usize);
        }
    });
    let c_del = ctrl.clone();
    screen.set_on_delete_index(move |idx| {
        if idx >= 0 {
            c_del.delete_visible_at(idx as usize);
        }
    });
    let c_clr = ctrl.clone();
    screen.set_on_clear_completed(move || {
        c_clr.clear_completed();
    });

    let root = ctrl.screen.build(cx, tree);
    (root, ctrl)
}

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Task Board".to_string(),
            width: 780.0,
            height: 620.0,
            ..Default::default()
        },
        |cx, tree, _| {
            let (root, _) = mount_todo_app(cx, tree);
            root
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui::reactive::reset_runtime;

    #[test]
    fn todo_crud_filters_search_empty_state_and_renders() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let (root, ctrl) = mount_todo_app(cx, &mut tree);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(780.0, 620.0),
            ..Default::default()
        };
        tree.layout(metrics);
        assert_eq!(ctrl.screen.visible_tasks.get().len(), 3);

        // Render initial list view
        let mut s1 = PaintScene::new();
        tree.paint(&mut s1);
        let textures = TextureRegistry::new();
        let rgba1 = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &s1,
            780,
            620,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );

        // Add new task via draft_title & trigger toggle + filter tabs
        ctrl.screen
            .draft_title
            .set("Verify keyboard focus traversal".into());
        (ctrl.screen.on_add_task.borrow_mut())();
        assert_eq!(ctrl.all_tasks.get().len(), 4);
        assert_eq!(ctrl.screen.visible_tasks.get().len(), 4);

        // Filter to Active (2 active initially + 1 new = 3)
        ctrl.screen.filter_tab.set(1);
        tree.layout(metrics);
        assert_eq!(ctrl.screen.visible_tasks.get().len(), 3);

        // Toggle first active item done
        ctrl.toggle_visible_at(0);
        tree.layout(metrics);
        assert_eq!(ctrl.screen.visible_tasks.get().len(), 2);

        // Search query yields empty-state `if/else` branch
        ctrl.screen.search_query.set("nonexistent-xyz".into());
        tree.layout(metrics);
        assert!(ctrl.screen.visible_tasks.get().is_empty());

        let mut s2 = PaintScene::new();
        tree.paint(&mut s2);
        let rgba2 = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &s2,
            780,
            620,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_ne!(rgba1, rgba2);

        // Delete item cleanly
        ctrl.screen.search_query.set(String::new());
        ctrl.screen.filter_tab.set(0);
        ctrl.delete_visible_at(0);
        assert_eq!(ctrl.all_tasks.get().len(), 3);
    }
}

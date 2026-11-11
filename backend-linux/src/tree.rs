// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Parsing of sway's window tree (`GET_TREE`) into window leaves.

use serde_json::Value;

use tpt_workspaces_core::model::{WindowGeometry, WindowId, WorkspaceId};

/// Sway's hidden scratchpad workspace. Windows there are not on any desktop.
pub const SCRATCHPAD: &str = "__i3_scratch";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leaf {
    pub id: WindowId,
    pub pid: u32,
    /// Wayland app id (native windows).
    pub app_id: Option<String>,
    /// X11 class (XWayland windows).
    pub class: Option<String>,
    pub title: String,
    pub workspace: WorkspaceId,
    pub geometry: WindowGeometry,
}

/// Every window leaf in the tree that sits on a real workspace.
pub fn leaves(root: &Value) -> Vec<Leaf> {
    let mut out = Vec::new();
    walk(root, None, &mut out);
    out
}

pub fn find(root: &Value, id: WindowId) -> Option<Leaf> {
    leaves(root).into_iter().find(|l| l.id == id)
}

fn walk(node: &Value, workspace: Option<&str>, out: &mut Vec<Leaf>) {
    let kind = node["type"].as_str().unwrap_or_default();
    let workspace = if kind == "workspace" {
        node["name"].as_str().or(workspace)
    } else {
        workspace
    };

    // A view is a con or floating_con that has a pid. Plain split containers have no pid.
    if matches!(kind, "con" | "floating_con") {
        if let (Some(ws), Some(id), Some(pid)) = (
            workspace,
            node["id"].as_i64().and_then(|v| u64::try_from(v).ok()),
            node["pid"].as_u64().and_then(|v| u32::try_from(v).ok()),
        ) {
            if ws != SCRATCHPAD && pid > 0 {
                out.push(Leaf {
                    id: WindowId(id),
                    pid,
                    app_id: node["app_id"].as_str().map(str::to_string),
                    class: node["window_properties"]["class"]
                        .as_str()
                        .map(str::to_string),
                    title: node["name"].as_str().unwrap_or_default().to_string(),
                    workspace: WorkspaceId::new(ws),
                    geometry: rect(&node["rect"]),
                });
            }
        }
    }

    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = node[key].as_array() {
            for child in children {
                walk(child, workspace, out);
            }
        }
    }
}

fn rect(v: &Value) -> WindowGeometry {
    let dim = |key: &str| v[key].as_i64().unwrap_or(0);
    WindowGeometry {
        x: i32::try_from(dim("x")).unwrap_or(0),
        y: i32::try_from(dim("y")).unwrap_or(0),
        width: u32::try_from(dim("width")).unwrap_or(0),
        height: u32::try_from(dim("height")).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "type": "root",
            "nodes": [{
                "type": "output",
                "nodes": [
                    {
                        "type": "workspace",
                        "name": "dev",
                        "nodes": [
                            {"type": "con", "id": 10, "pid": 1234, "app_id": "foot",
                             "name": "shell", "rect": {"x": 0, "y": 0, "width": 800, "height": 600},
                             "nodes": [], "floating_nodes": []},
                            {"type": "con", "id": 11, "pid": null, "name": "split",
                             "nodes": [{"type": "con", "id": 12, "pid": 2000,
                                        "window_properties": {"class": "Xwayland"}, "name": "x",
                                        "rect": {"x": 1, "y": 2, "width": 3, "height": 4}}],
                             "floating_nodes": []}
                        ],
                        "floating_nodes": [
                            {"type": "floating_con", "id": 13, "pid": 3000, "app_id": "float",
                             "name": "dialog", "rect": {"x": 5, "y": 6, "width": 7, "height": 8}}
                        ]
                    },
                    {
                        "type": "workspace",
                        "name": "__i3_scratch",
                        "nodes": [{"type": "con", "id": 99, "pid": 4000, "app_id": "hidden",
                                   "name": "h", "rect": {"x": 0, "y": 0, "width": 1, "height": 1}}],
                        "floating_nodes": []
                    }
                ],
                "floating_nodes": []
            }]
        })
    }

    #[test]
    fn finds_views_with_their_workspace() {
        let found = leaves(&sample());
        let ids: Vec<u64> = found.iter().map(|l| l.id.0).collect();
        assert_eq!(ids, vec![10, 12, 13]);
        assert!(found.iter().take(2).all(|l| l.workspace.as_str() == "dev"));
    }

    #[test]
    fn skips_scratchpad_and_split_containers() {
        let found = leaves(&sample());
        assert!(found.iter().all(|l| l.id.0 != 11 && l.id.0 != 99));
    }

    #[test]
    fn reads_app_id_class_and_geometry() {
        let found = leaves(&sample());
        let term = &found[0];
        assert_eq!(term.pid, 1234);
        assert_eq!(term.app_id.as_deref(), Some("foot"));
        assert_eq!(term.geometry.width, 800);
        let xwl = &found[1];
        assert_eq!(xwl.class.as_deref(), Some("Xwayland"));
        assert_eq!(xwl.app_id, None);
    }

    #[test]
    fn find_by_id() {
        assert_eq!(find(&sample(), WindowId(13)).unwrap().pid, 3000);
        assert!(find(&sample(), WindowId(404)).is_none());
    }
}

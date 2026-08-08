//! Native macOS application menus.

use gpui::{App, KeyBinding, Menu, MenuItem, SystemMenuType, actions};

use crate::app::shell::{
    AddWorkspace, CloseTab, CommandPalette, CommitPush, FindInFile, FindNext, FindPrev,
    FindReplace, NavigateBack, NavigateForward, NewTerminal, NextWorkspace, QuickOpen,
    RevealInExplorer, SaveFile, SearchAllFiles, ToggleAppearance, ToggleFocusMode, ToggleInspector,
    TogglePreview, ToggleSidebar, ToggleSidebarTab, ToggleWrap, Workspace1, Workspace2, Workspace3,
    Workspace4, Workspace5, Workspace6, Workspace7, Workspace8, Workspace9, ZoomIn, ZoomOut,
};

actions!(
    artifex_menu,
    [
        Hide,
        HideOthers,
        ShowAll,
        Quit,
        Minimize,
        Zoom,
        ToggleFullScreen
    ]
);

/// Registers global application actions, their shortcuts, and the native menu bar.
pub fn init(cx: &mut App) {
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Minimize, cx| {
        if let Some(handle) = cx.active_window() {
            let _ = handle.update(cx, |_, window, _| window.minimize_window());
        }
    });
    cx.on_action(|_: &Zoom, cx| {
        if let Some(handle) = cx.active_window() {
            let _ = handle.update(cx, |_, window, _| window.zoom_window());
        }
    });
    cx.on_action(|_: &ToggleFullScreen, cx| {
        if let Some(handle) = cx.active_window() {
            let _ = handle.update(cx, |_, window, _| window.toggle_fullscreen());
        }
    });

    cx.bind_keys([
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
    ]);
    cx.set_menus(app_menus());
}

fn app_menus() -> Vec<Menu> {
    vec![
        Menu::new("Artifex").items([
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Artifex", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit Artifex", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Open Folder...", AddWorkspace),
            MenuItem::action("New Terminal", NewTerminal),
            MenuItem::separator(),
            MenuItem::action("Save", SaveFile),
            MenuItem::action("Close Tab", CloseTab),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Find", FindInFile),
            MenuItem::action("Find and Replace", FindReplace),
            MenuItem::action("Find Next", FindNext),
            MenuItem::action("Find Previous", FindPrev),
            MenuItem::separator(),
            MenuItem::action("Search All Files...", SearchAllFiles),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Toggle Inspector", ToggleInspector),
            MenuItem::action("Toggle Explorer and Git", ToggleSidebarTab),
            MenuItem::action("Toggle Focus Mode", ToggleFocusMode),
            MenuItem::separator(),
            MenuItem::action("Toggle Source and Preview", TogglePreview),
            MenuItem::action("Toggle Word Wrap", ToggleWrap),
            MenuItem::action("Toggle Light and Dark", ToggleAppearance),
            MenuItem::separator(),
            MenuItem::action("Zoom In", ZoomIn),
            MenuItem::action("Zoom Out", ZoomOut),
        ]),
        Menu::new("Go").items([
            MenuItem::action("Back", NavigateBack),
            MenuItem::action("Forward", NavigateForward),
            MenuItem::separator(),
            MenuItem::action("Quick Open...", QuickOpen),
            MenuItem::action("Command Palette...", CommandPalette),
            MenuItem::action("Reveal Active File in Explorer", RevealInExplorer),
        ]),
        Menu::new("Workspaces").items([
            MenuItem::action("Open New Workspace...", AddWorkspace),
            MenuItem::action("Next Workspace", NextWorkspace),
            MenuItem::separator(),
            MenuItem::action("Workspace 1", Workspace1),
            MenuItem::action("Workspace 2", Workspace2),
            MenuItem::action("Workspace 3", Workspace3),
            MenuItem::action("Workspace 4", Workspace4),
            MenuItem::action("Workspace 5", Workspace5),
            MenuItem::action("Workspace 6", Workspace6),
            MenuItem::action("Workspace 7", Workspace7),
            MenuItem::action("Workspace 8", Workspace8),
            MenuItem::action("Workspace 9", Workspace9),
        ]),
        Menu::new("Git").items([MenuItem::action("Commit and Push", CommitPush)]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
            MenuItem::separator(),
            MenuItem::action("Enter Full Screen", ToggleFullScreen),
        ]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_menu_order_matches_the_application_contract() {
        let menus = app_menus();
        let names: Vec<&str> = menus.iter().map(|menu| menu.name.as_ref()).collect();

        assert_eq!(
            names,
            [
                "Artifex",
                "File",
                "Edit",
                "View",
                "Go",
                "Workspaces",
                "Git",
                "Window",
            ]
        );
    }

    #[test]
    fn workspaces_menu_exposes_next_workspace_action() {
        let menus = app_menus();
        let names: Vec<&str> = menus
            .iter()
            .filter(|menu| menu.name.as_ref() == "Workspaces")
            .flat_map(|menu| menu.items.iter())
            .filter_map(|item| match item {
                MenuItem::Action { name, .. } => Some(name.as_ref()),
                _ => None,
            })
            .collect();

        assert!(names.contains(&"Next Workspace"));
    }
}

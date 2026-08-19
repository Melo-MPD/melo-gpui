mod actions;
mod assets;
mod dock_icon;
mod mpd;
mod perf;
mod state;
mod theme;
mod ui;

use actions::*;
use assets::Assets;
use gpui::{
    App, Application, Bounds, KeyBinding, Menu, MenuItem, SharedString, SystemMenuType,
    TitlebarOptions, WindowBounds, WindowOptions, point, prelude::*, px, size,
};
use state::AppState;
use ui::root::MeloRoot;
use ui::widgets::text_input;

fn main() {
    perf::init();
    if dock_icon::maybe_handle_cli() {
        return;
    }
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        dock_icon::install();
        cx.activate(true);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &About, cx| ui::about::open(cx));
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.bind_keys(text_input::key_bindings());
        let root = Some("Melo && !TextInput");
        cx.bind_keys([
            KeyBinding::new("space", TogglePlayPause, root),
            KeyBinding::new("cmd-right", NextTrack, root),
            KeyBinding::new("cmd-left", PrevTrack, root),
            KeyBinding::new("cmd-up", VolumeUp, Some("Melo")),
            KeyBinding::new("cmd-down", VolumeDown, Some("Melo")),
            KeyBinding::new("cmd-1", ShowNowPlaying, Some("Melo")),
            KeyBinding::new("cmd-2", ShowQueue, Some("Melo")),
            KeyBinding::new("cmd-3", ShowLibrary, Some("Melo")),
            KeyBinding::new("cmd-,", ShowSettings, Some("Melo")),
            KeyBinding::new("escape", Escape, Some("Melo && !TextInput")),
            KeyBinding::new("cmd-q", Quit, None),
        ]);

        cx.set_menus(vec![
            Menu {
                name: "Melo".into(),
                items: vec![
                    MenuItem::action("About Melo", About),
                    MenuItem::separator(),
                    MenuItem::action("Settings…", ShowSettings),
                    MenuItem::separator(),
                    MenuItem::os_submenu("Services", SystemMenuType::Services),
                    MenuItem::separator(),
                    MenuItem::action("Quit Melo", Quit),
                ],
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::action("Cut", text_input::Cut),
                    MenuItem::action("Copy", text_input::Copy),
                    MenuItem::action("Paste", text_input::Paste),
                    MenuItem::separator(),
                    MenuItem::action("Select All", text_input::SelectAll),
                ],
            },
            Menu {
                name: "View".into(),
                items: vec![
                    MenuItem::action("Now Playing", ShowNowPlaying),
                    MenuItem::action("Queue", ShowQueue),
                    MenuItem::action("Library", ShowLibrary),
                    MenuItem::separator(),
                    MenuItem::action("Settings", ShowSettings),
                ],
            },
            Menu {
                name: "Playback".into(),
                items: vec![
                    MenuItem::action("Play / Pause", TogglePlayPause),
                    MenuItem::action("Next Track", NextTrack),
                    MenuItem::action("Previous Track", PrevTrack),
                    MenuItem::separator(),
                    MenuItem::action("Volume Up", VolumeUp),
                    MenuItem::action("Volume Down", VolumeDown),
                ],
            },
        ]);

        perf::start_frame_probe(cx);
        let state = cx.new(AppState::new);

        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("Melo")),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.), px(12.))),
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(920.), px(620.))),
                ..Default::default()
            },
            |window, cx| {
                let root = cx.new(|cx| MeloRoot::new(state.clone(), window, cx));
                let handle = root.read(cx).focus_handle().clone();
                window.focus(&handle);
                root
            },
        )
        .expect("open main window");
    });
}

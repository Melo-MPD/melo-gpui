use gpui::actions;

actions!(
    melo,
    [
        TogglePlayPause,
        NextTrack,
        PrevTrack,
        VolumeUp,
        VolumeDown,
        ShowNowPlaying,
        ShowQueue,
        ShowLibrary,
        ShowSettings,
        Escape,
        About,
        Quit,
    ]
);

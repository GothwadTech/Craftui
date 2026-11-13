//! Built-in Material Symbols vector icons for CraftUI (`"std/icons.cui"`).

pub const STD_ICONS_SOURCE: &str = r#"
export component IconSearch {
    in property <length> size: 24dp;
    in property <color> color: Theme.text_primary;

    Icon {
        size: root.size;
        color: root.color;
        svg: "<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M15.5 14h-.79l-.28-.27A6.471 6.471 0 0 0 16 9.5 6.5 6.5 0 1 0 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z\"/></svg>";
    }
}

export component IconClose {
    in property <length> size: 24dp;
    in property <color> color: Theme.text_primary;

    Icon {
        size: root.size;
        color: root.color;
        svg: "<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z\"/></svg>";
    }
}

export component IconArrowBack {
    in property <length> size: 24dp;
    in property <color> color: Theme.text_primary;

    Icon {
        size: root.size;
        color: root.color;
        svg: "<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M20 11H7.83l5.59-5.59L12 4l-8 8 8 8 1.41-1.41L7.83 13H20v-2z\"/></svg>";
    }
}

export component IconArrowForward {
    in property <length> size: 24dp;
    in property <color> color: Theme.text_primary;

    Icon {
        size: root.size;
        color: root.color;
        svg: "<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M12 4l-1.41 1.41L16.17 11H4v2h12.17l-5.58 5.59L12 20l8-8z\"/></svg>";
    }
}

export component IconMenu {
    in property <length> size: 24dp;
    in property <color> color: Theme.text_primary;

    Icon {
        size: root.size;
        color: root.color;
        svg: "<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M3 18h18v-2H3v2zm0-5h18v-2H3v2zm0-7v2h18V6H3z\"/></svg>";
    }
}

export component IconCheck {
    in property <length> size: 24dp;
    in property <color> color: Theme.text_primary;

    Icon {
        size: root.size;
        color: root.color;
        svg: "<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z\"/></svg>";
    }
}

export component IconAdd {
    in property <length> size: 24dp;
    in property <color> color: Theme.text_primary;

    Icon {
        size: root.size;
        color: root.color;
        svg: "<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z\"/></svg>";
    }
}

export component IconSettings {
    in property <length> size: 24dp;
    in property <color> color: Theme.text_primary;

    Icon {
        size: root.size;
        color: root.color;
        svg: "<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 0 0 .12-.61l-1.92-3.32a.488.488 0 0 0-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54A.484.484 0 0 0 14 2h-4c-.25 0-.46.18-.49.42l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.63 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58a.49.49 0 0 0-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h4c.25 0 .46-.18.49-.42l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z\"/></svg>";
    }
}

export component IconFavorite {
    in property <length> size: 24dp;
    in property <color> color: Theme.text_primary;

    Icon {
        size: root.size;
        color: root.color;
        svg: "<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z\"/></svg>";
    }
}

export component IconShare {
    in property <length> size: 24dp;
    in property <color> color: Theme.text_primary;

    Icon {
        size: root.size;
        color: root.color;
        svg: "<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M18 16.08c-.76 0-1.44.3-1.96.77L8.91 12.7c.05-.23.09-.46.09-.7s-.04-.47-.09-.7l7.05-4.11c.54.5 1.25.81 2.04.81 1.66 0 3-1.34 3-3s-1.34-3-3-3-3 1.34-3 3c0 .24.04.47.09.7L8.04 9.81C7.5 9.31 6.79 9 6 9c-1.66 0-3 1.34-3 3s1.34 3 3 3c.79 0 1.5-.31 2.04-.81l7.12 4.16c-.05.21-.08.43-.08.65 0 1.61 1.31 2.92 2.92 2.92 1.61 0 2.92-1.31 2.92-2.92s-1.31-2.92-2.92-2.92z\"/></svg>";
    }
}

export component IconRefresh {
    in property <length> size: 24dp;
    in property <color> color: Theme.text_primary;

    Icon {
        size: root.size;
        color: root.color;
        svg: "<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M17.65 6.35A7.958 7.958 0 0 0 12 4c-4.42 0-7.99 3.58-7.99 8s3.57 8 7.99 8c3.73 0 6.84-2.55 7.73-6h-2.08A5.99 5.99 0 0 1 12 18c-3.31 0-6-2.69-6-6s2.69-6 6-6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z\"/></svg>";
    }
}

export component IconMoreVert {
    in property <length> size: 24dp;
    in property <color> color: Theme.text_primary;

    Icon {
        size: root.size;
        color: root.color;
        svg: "<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M12 8c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zm0 2c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm0 6c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z\"/></svg>";
    }
}
"#;

pub const STD_ICONS_EXPORTS: &[&str] = &[
    "IconSearch",
    "IconClose",
    "IconArrowBack",
    "IconArrowForward",
    "IconMenu",
    "IconCheck",
    "IconAdd",
    "IconSettings",
    "IconFavorite",
    "IconShare",
    "IconRefresh",
    "IconMoreVert",
];

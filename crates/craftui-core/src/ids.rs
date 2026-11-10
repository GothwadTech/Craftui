use bitflags::bitflags;
use slotmap::new_key_type;

new_key_type! {
    /// Generational ID for a node in the retained widget & layout tree.
    pub struct NodeId;
    /// Generational ID for a reactive ownership scope.
    pub struct ScopeId;
    /// Generational ID for a GPU/CPU image or pixel-buffer texture.
    pub struct TextureId;
    /// Generational ID for an embedded OS-native PlatformView surface.
    pub struct PlatformViewId;
}

bitflags! {
    /// Fine-grained invalidation flags per retained widget node.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct DirtyFlags: u8 {
        const NONE   = 0;
        const LAYOUT = 1 << 0;
        const PAINT  = 1 << 1;
        const A11Y   = 1 << 2;
        const TEXT   = 1 << 3;
        const ALL    = Self::LAYOUT.bits() | Self::PAINT.bits() | Self::A11Y.bits() | Self::TEXT.bits();
    }
}

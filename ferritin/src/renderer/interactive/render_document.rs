use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
};

use super::state::InteractiveState;
use crate::styled_string::NodePath;

// Baseline left margin for all content - provides breathing room and space for outdented borders
pub(super) const BASELINE_LEFT_MARGIN: u16 = 3;

impl<'a> InteractiveState<'a> {
    /// Render document nodes to buffer, updating action map
    pub(super) fn render_document(&mut self, _area: Rect, buf: &mut Buffer) {
        self.render_cache.actions.clear();

        // Layout state already initialized in render_frame with area
        // Set initial position and indent
        self.layout.pos = Position {
            x: BASELINE_LEFT_MARGIN,
            y: 0,
        };
        self.layout.indent = BASELINE_LEFT_MARGIN;

        // Use raw pointer to avoid borrow checker issues when calling render_node
        let nodes_ptr = self.document.document.nodes.as_ptr();
        let node_count = self.document.document.nodes.len();

        for idx in 0..node_count {
            if self.layout.pos.y >= self.layout.area.height + self.viewport.scroll_offset {
                break; // Past visible area
            }

            // Add blank line between consecutive top-level blocks
            if idx > 0 {
                self.layout.pos.y += 1;
            }

            // Update path for this top-level node
            self.layout.node_path = NodePath::new();
            self.layout.node_path.push(idx);

            // SAFETY: idx is bounded by node_count, and nodes_ptr is valid for the duration of this method
            let node = unsafe { &*nodes_ptr.add(idx) };
            self.render_node(node, buf);
        }
    }
}

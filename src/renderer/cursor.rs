//! 光标与选区系统

/// 光标位置
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorPosition {
    /// 字符偏移（在文本中的位置）
    pub offset: usize,
}

/// 文本选区
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSelection {
    pub start: usize,
    pub end: usize,
}

impl TextSelection {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
    pub fn normalized(&self) -> (usize, usize) {
        (self.start.min(self.end), self.start.max(self.end))
    }
}

/// 光标渲染器
pub struct CursorRenderer {
    /// 当前光标位置
    pub position: CursorPosition,
    /// 选区
    pub selection: Option<TextSelection>,
    /// 闪烁计时（秒）
    blink_timer: f32,
    /// 光标是否可见
    pub visible: bool,
}

impl CursorRenderer {
    pub fn new() -> Self {
        Self {
            position: CursorPosition { offset: 0 },
            selection: None,
            blink_timer: 0.0,
            visible: true,
        }
    }

    /// 更新闪烁状态（每帧调用）
    pub fn update(&mut self, dt: f32) {
        self.blink_timer += dt;
        if self.blink_timer >= 0.53 {
            // 标准光标闪烁间隔 530ms
            self.blink_timer = 0.0;
            self.visible = !self.visible;
        }
    }

    /// 重置光标到文本末尾
    pub fn reset(&mut self, text_len: usize) {
        self.position.offset = text_len;
        self.selection = None;
        self.blink_timer = 0.0;
        self.visible = true;
    }

    /// 向左移动一个字符
    pub fn move_left(&mut self) {
        if self.position.offset > 0 {
            self.position.offset -= 1;
        }
        self.blink_timer = 0.0;
        self.visible = true;
    }

    /// 向右移动一个字符
    pub fn move_right(&mut self, max: usize) {
        if self.position.offset < max {
            self.position.offset += 1;
        }
        self.blink_timer = 0.0;
        self.visible = true;
    }

    /// 获取光标渲染的 x 坐标（基于字符宽度）
    pub fn cursor_x(&self, char_width: f32) -> f32 {
        self.position.offset as f32 * char_width
    }
}

impl Default for CursorRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_renderer_new() {
        let cursor = CursorRenderer::new();
        assert_eq!(cursor.position.offset, 0);
        assert!(cursor.visible);
        assert!(cursor.selection.is_none());
    }

    #[test]
    fn test_cursor_renderer_default() {
        let cursor = CursorRenderer::default();
        assert_eq!(cursor.position.offset, 0);
        assert!(cursor.visible);
    }

    #[test]
    fn test_cursor_update_toggles_visibility() {
        let mut cursor = CursorRenderer::new();
        assert!(cursor.visible);

        // 更新 0.53 秒，应该切换可见性
        cursor.update(0.53);
        assert!(!cursor.visible);

        // 再更新 0.53 秒，恢复可见
        cursor.update(0.53);
        assert!(cursor.visible);
    }

    #[test]
    fn test_cursor_update_partial_dt() {
        let mut cursor = CursorRenderer::new();
        // 小于 0.53，不应该切换
        cursor.update(0.3);
        assert!(cursor.visible);

        // 累积到达 0.6 > 0.53，应该切换
        cursor.update(0.3);
        assert!(!cursor.visible);
    }

    #[test]
    fn test_cursor_update_multiple_blinks() {
        let mut cursor = CursorRenderer::new();
        for _ in 0..10 {
            cursor.update(0.53);
        }
        // 10 次切换后，偶数次切换回到可见
        assert!(cursor.visible);
    }

    #[test]
    fn test_cursor_update_large_dt() {
        let mut cursor = CursorRenderer::new();
        // 一次大的 dt 只切换一次
        cursor.update(2.0);
        assert!(!cursor.visible);
    }

    #[test]
    fn test_cursor_reset() {
        let mut cursor = CursorRenderer::new();
        cursor.position.offset = 5;
        cursor.visible = false;
        cursor.selection = Some(TextSelection::new(2, 8));

        cursor.reset(10);
        assert_eq!(cursor.position.offset, 10);
        assert!(cursor.visible);
        assert!(cursor.selection.is_none());
    }

    #[test]
    fn test_cursor_reset_zero() {
        let mut cursor = CursorRenderer::new();
        cursor.position.offset = 5;
        cursor.reset(0);
        assert_eq!(cursor.position.offset, 0);
    }

    #[test]
    fn test_cursor_move_left() {
        let mut cursor = CursorRenderer::new();
        cursor.position.offset = 5;
        cursor.visible = false;

        cursor.move_left();
        assert_eq!(cursor.position.offset, 4);
        assert!(cursor.visible); // 重置可见
    }

    #[test]
    fn test_cursor_move_left_at_zero() {
        let mut cursor = CursorRenderer::new();
        cursor.position.offset = 0;
        cursor.move_left();
        assert_eq!(cursor.position.offset, 0);
    }

    #[test]
    fn test_cursor_move_right() {
        let mut cursor = CursorRenderer::new();
        cursor.position.offset = 3;
        cursor.visible = false;

        cursor.move_right(10);
        assert_eq!(cursor.position.offset, 4);
        assert!(cursor.visible);
    }

    #[test]
    fn test_cursor_move_right_at_max() {
        let mut cursor = CursorRenderer::new();
        cursor.position.offset = 10;
        cursor.move_right(10);
        assert_eq!(cursor.position.offset, 10);
    }

    #[test]
    fn test_cursor_move_right_empty() {
        let mut cursor = CursorRenderer::new();
        cursor.move_right(0);
        assert_eq!(cursor.position.offset, 0);
    }

    #[test]
    fn test_cursor_move_resets_blink_timer() {
        let mut cursor = CursorRenderer::new();
        cursor.update(0.5); // 接近切换但还没切换
        cursor.visible = false;

        cursor.move_left();
        assert!(cursor.visible); // 移动后立刻可见

        // 再经过小 dt 不应该切换（因为计时器已重置）
        cursor.update(0.1);
        assert!(cursor.visible);
    }

    #[test]
    fn test_cursor_x() {
        let cursor = CursorRenderer::new();
        assert_eq!(cursor.cursor_x(8.0), 0.0);

        let mut cursor = CursorRenderer::new();
        cursor.position.offset = 5;
        assert_eq!(cursor.cursor_x(8.0), 40.0);
    }

    #[test]
    fn test_cursor_x_custom_char_width() {
        let mut cursor = CursorRenderer::new();
        cursor.position.offset = 3;
        assert_eq!(cursor.cursor_x(10.0), 30.0);
        assert_eq!(cursor.cursor_x(16.0), 48.0);
    }

    #[test]
    fn test_text_selection_new() {
        let sel = TextSelection::new(5, 10);
        assert_eq!(sel.start, 5);
        assert_eq!(sel.end, 10);
    }

    #[test]
    fn test_text_selection_is_empty() {
        let sel = TextSelection::new(5, 5);
        assert!(sel.is_empty());

        let sel = TextSelection::new(5, 6);
        assert!(!sel.is_empty());
    }

    #[test]
    fn test_text_selection_normalized() {
        let sel = TextSelection::new(10, 5);
        let (min, max) = sel.normalized();
        assert_eq!(min, 5);
        assert_eq!(max, 10);
    }

    #[test]
    fn test_text_selection_normalized_already_ordered() {
        let sel = TextSelection::new(3, 8);
        let (min, max) = sel.normalized();
        assert_eq!(min, 3);
        assert_eq!(max, 8);
    }

    #[test]
    fn test_text_selection_normalized_equal() {
        let sel = TextSelection::new(7, 7);
        let (min, max) = sel.normalized();
        assert_eq!(min, 7);
        assert_eq!(max, 7);
    }

    #[test]
    fn test_cursor_preserves_selection_on_move() {
        let mut cursor = CursorRenderer::new();
        cursor.position.offset = 5;
        cursor.selection = Some(TextSelection::new(2, 8));

        cursor.move_left();
        assert!(cursor.selection.is_some());
        assert_eq!(cursor.selection.unwrap().start, 2);
    }

    #[test]
    fn test_cursor_update_accumulates_time() {
        let mut cursor = CursorRenderer::new();
        // 累积 0.53 秒
        cursor.update(0.2);
        assert!(cursor.visible);
        cursor.update(0.2);
        assert!(cursor.visible);
        cursor.update(0.2); // 0.6 > 0.53
        assert!(!cursor.visible);

        // 累积剩余时间继续
        cursor.update(0.4); // 0.4 < 0.53，不应切换
        assert!(!cursor.visible);
        cursor.update(0.2); // 0.6 > 0.53，切换
        assert!(cursor.visible);
    }

    #[test]
    fn test_cursor_move_right_after_left() {
        let mut cursor = CursorRenderer::new();
        cursor.position.offset = 5;

        cursor.move_left(); // -> 4
        cursor.move_right(10); // -> 5
        assert_eq!(cursor.position.offset, 5);

        cursor.move_right(10); // -> 6
        assert_eq!(cursor.position.offset, 6);
    }

    #[test]
    fn test_cursor_reset_clears_blink_accumulation() {
        let mut cursor = CursorRenderer::new();
        cursor.update(0.5);
        cursor.reset(3);
        assert!(cursor.visible);
        // 计时器应该被清零
        cursor.update(0.5);
        assert!(cursor.visible); // 还不到 0.53
        cursor.update(0.03);
        assert!(!cursor.visible); // 累积超过 0.53
    }
}

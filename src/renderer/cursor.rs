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

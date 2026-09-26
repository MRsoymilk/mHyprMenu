use crate::{config::Config, style::Style};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Root(usize),
    Child { root: usize, child: usize },
}

#[derive(Debug, Clone)]
pub enum ClickOutcome {
    Keep,
    Close,
    Command(String),
}

pub struct MenuState {
    pub config: Config,
    pub style: Style,
    pub hovered: Option<Hit>,
    pub open_root: Option<usize>,
    origin: Option<(f64, f64)>,
}

fn wrap_index(index: usize, count: usize, delta: i32) -> usize {
    if delta < 0 {
        if index == 0 { count - 1 } else { index - 1 }
    } else if index + 1 >= count {
        0
    } else {
        index + 1
    }
}

impl MenuState {
    pub fn new(config: Config, style: Style) -> Self {
        Self {
            config,
            style,
            hovered: None,
            open_root: None,
            origin: None,
        }
    }

    pub fn reset(&mut self) {
        self.hovered = None;
        self.open_root = None;
        self.origin = None;
    }

    pub fn replace_config(&mut self, config: Config, style: Style) {
        self.config = config;
        self.style = style;
        self.reset();
    }

    pub fn has_origin(&self) -> bool {
        self.origin.is_some()
    }

    pub fn set_origin(&mut self, x: f64, y: f64) {
        self.origin = Some((x, y));
        self.hovered = None;
        self.open_root = None;
    }

    pub fn move_vertical(&mut self, delta: i32) -> bool {
        match self.hovered {
            Some(Hit::Child { root, child }) => {
                let count = self.config.items[root].children.len();
                if count == 0 {
                    return false;
                }

                let next = wrap_index(child, count, delta);
                self.hovered = Some(Hit::Child { root, child: next });
                self.open_root = Some(root);
                true
            }
            Some(Hit::Root(index)) => {
                let count = self.config.items.len();
                if count == 0 {
                    return false;
                }

                let next = wrap_index(index, count, delta);
                self.hovered = Some(Hit::Root(next));
                self.open_root = None;
                true
            }
            None => {
                let count = self.config.items.len();
                if count == 0 {
                    return false;
                }

                let index = if delta < 0 { count - 1 } else { 0 };
                self.hovered = Some(Hit::Root(index));
                self.open_root = None;
                true
            }
        }
    }

    pub fn move_right(&mut self) -> bool {
        match self.hovered {
            Some(Hit::Root(root)) => {
                if self.config.items[root].children.is_empty() {
                    return false;
                }

                self.open_root = Some(root);
                self.hovered = Some(Hit::Child { root, child: 0 });
                true
            }
            Some(Hit::Child { .. }) => false,
            None => {
                if self.config.items.is_empty() {
                    return false;
                }

                self.hovered = Some(Hit::Root(0));
                self.open_root = None;
                true
            }
        }
    }

    pub fn move_left(&mut self) -> bool {
        match self.hovered {
            Some(Hit::Child { root, .. }) => {
                self.hovered = Some(Hit::Root(root));
                self.open_root = None;
                true
            }
            _ => false,
        }
    }

    pub fn activate_selected(&mut self) -> ClickOutcome {
        match self.hovered {
            Some(Hit::Root(root)) => {
                let item = &self.config.items[root];
                if !item.children.is_empty() {
                    self.open_root = Some(root);
                    self.hovered = Some(Hit::Child { root, child: 0 });
                    ClickOutcome::Keep
                } else {
                    item.command
                        .as_ref()
                        .map(|command| ClickOutcome::Command(command.clone()))
                        .unwrap_or(ClickOutcome::Keep)
                }
            }
            Some(Hit::Child { root, child }) => {
                let item = &self.config.items[root].children[child];
                item.command
                    .as_ref()
                    .map(|command| ClickOutcome::Command(command.clone()))
                    .unwrap_or(ClickOutcome::Keep)
            }
            None => ClickOutcome::Keep,
        }
    }

    pub fn pointer_moved(&mut self, x: f64, y: f64, surface_w: f64, surface_h: f64) -> bool {
        let mut changed = false;

        if self.origin.is_none() {
            self.origin = Some((x, y));
            changed = true;
        }

        let hit = self.hit_test(x, y, surface_w, surface_h);
        if hit != self.hovered {
            self.hovered = hit;
            changed = true;
        }

        match hit {
            Some(Hit::Root(index)) => {
                let next_open = (!self.config.items[index].children.is_empty()).then_some(index);
                if self.open_root != next_open {
                    self.open_root = next_open;
                    changed = true;
                }
            }
            Some(Hit::Child { root, .. }) => {
                if self.open_root != Some(root) {
                    self.open_root = Some(root);
                    changed = true;
                }
            }
            None => {
                if self.open_root.is_some() {
                    self.open_root = None;
                    changed = true;
                }
            }
        }

        changed
    }

    pub fn click(&self, x: f64, y: f64, surface_w: f64, surface_h: f64) -> ClickOutcome {
        match self.hit_test(x, y, surface_w, surface_h) {
            Some(Hit::Root(index)) => {
                let item = &self.config.items[index];
                if !item.children.is_empty() {
                    ClickOutcome::Keep
                } else if let Some(command) = &item.command {
                    ClickOutcome::Command(command.clone())
                } else {
                    ClickOutcome::Keep
                }
            }
            Some(Hit::Child { root, child }) => {
                let item = &self.config.items[root].children[child];
                item.command
                    .as_ref()
                    .map(|command| ClickOutcome::Command(command.clone()))
                    .unwrap_or(ClickOutcome::Keep)
            }
            None => ClickOutcome::Close,
        }
    }

    pub fn root_rect(&self, surface_w: f64, surface_h: f64) -> Option<Rect> {
        let (origin_x, origin_y) = self.origin?;
        let w = self.style.menu.width.min(surface_w.max(1.0));
        let h =
            (self.config.items.len() as f64 * self.style.menu.item_height).min(surface_h.max(1.0));

        Some(Rect {
            x: origin_x.clamp(0.0, (surface_w - w).max(0.0)),
            y: origin_y.clamp(0.0, (surface_h - h).max(0.0)),
            w,
            h,
        })
    }

    pub fn root_item_rect(&self, index: usize, surface_w: f64, surface_h: f64) -> Option<Rect> {
        let root = self.root_rect(surface_w, surface_h)?;
        Some(Rect {
            x: root.x,
            y: root.y + index as f64 * self.style.menu.item_height,
            w: root.w,
            h: self.style.menu.item_height,
        })
    }

    pub fn submenu_rect(&self, surface_w: f64, surface_h: f64) -> Option<Rect> {
        let root_index = self.open_root?;
        let parent = self.root_item_rect(root_index, surface_w, surface_h)?;
        let count = self.config.items[root_index].children.len();
        if count == 0 {
            return None;
        }

        let w = self.style.menu.width.min(surface_w.max(1.0));
        let h = (count as f64 * self.style.menu.item_height).min(surface_h.max(1.0));
        let x = if parent.x + parent.w + w <= surface_w {
            parent.x + parent.w
        } else {
            (parent.x - w).max(0.0)
        };

        Some(Rect {
            x,
            y: parent.y.clamp(0.0, (surface_h - h).max(0.0)),
            w,
            h,
        })
    }

    pub fn child_item_rect(&self, child: usize, surface_w: f64, surface_h: f64) -> Option<Rect> {
        let submenu = self.submenu_rect(surface_w, surface_h)?;
        Some(Rect {
            x: submenu.x,
            y: submenu.y + child as f64 * self.style.menu.item_height,
            w: submenu.w,
            h: self.style.menu.item_height,
        })
    }

    fn hit_test(&self, x: f64, y: f64, surface_w: f64, surface_h: f64) -> Option<Hit> {
        if let Some(root_index) = self.open_root
            && let Some(submenu) = self.submenu_rect(surface_w, surface_h)
            && submenu.contains(x, y)
        {
            let child = ((y - submenu.y) / self.style.menu.item_height) as usize;
            if child < self.config.items[root_index].children.len() {
                return Some(Hit::Child {
                    root: root_index,
                    child,
                });
            }
        }

        let root = self.root_rect(surface_w, surface_h)?;
        if root.contains(x, y) {
            let index = ((y - root.y) / self.style.menu.item_height) as usize;
            if index < self.config.items.len() {
                return Some(Hit::Root(index));
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> MenuState {
        let config = Config::load_from("config.example.toml").expect("load example config");
        let style = Style::load_from("style.example.toml").expect("load example style");
        let mut menu = MenuState::new(config, style);
        menu.set_origin(100.0, 100.0);
        menu
    }

    #[test]
    fn vertical_navigation_wraps_root_items() {
        let mut menu = menu();

        assert!(menu.move_vertical(1));
        assert_eq!(menu.hovered, Some(Hit::Root(0)));

        assert!(menu.move_vertical(-1));
        assert_eq!(menu.hovered, Some(Hit::Root(menu.config.items.len() - 1)));
    }

    #[test]
    fn right_and_left_move_between_parent_and_submenu() {
        let mut menu = menu();
        let root = menu
            .config
            .items
            .iter()
            .position(|item| !item.children.is_empty())
            .expect("default menu should contain a submenu");

        menu.hovered = Some(Hit::Root(root));

        assert!(menu.move_right());
        assert_eq!(menu.open_root, Some(root));
        assert_eq!(menu.hovered, Some(Hit::Child { root, child: 0 }));

        assert!(menu.move_left());
        assert_eq!(menu.open_root, None);
        assert_eq!(menu.hovered, Some(Hit::Root(root)));
    }

    #[test]
    fn enter_on_leaf_returns_command() {
        let mut menu = menu();
        menu.hovered = Some(Hit::Root(0));

        match menu.activate_selected() {
            ClickOutcome::Command(command) => assert_eq!(command, "alacritty"),
            other => panic!("expected command, got {other:?}"),
        }
    }

    #[test]
    fn enter_on_submenu_opens_first_child() {
        let mut menu = menu();
        let root = menu
            .config
            .items
            .iter()
            .position(|item| !item.children.is_empty())
            .expect("default menu should contain a submenu");

        menu.hovered = Some(Hit::Root(root));

        assert!(matches!(menu.activate_selected(), ClickOutcome::Keep));
        assert_eq!(menu.open_root, Some(root));
        assert_eq!(menu.hovered, Some(Hit::Child { root, child: 0 }));
    }
}

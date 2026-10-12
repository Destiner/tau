use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandSource {
    Extension,
    Prompt,
    Skill,
}
impl CommandSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extension => "extension",
            Self::Prompt => "prompt",
            Self::Skill => "skill",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandScope {
    User,
    Project,
    Temporary,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandOrigin {
    Package,
    TopLevel,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSourceInfo {
    pub path: String,
    pub source: String,
    pub scope: CommandScope,
    pub origin: CommandOrigin,
    pub base_dir: Option<String>,
}
/// Pi get_commands options, in Pi order. Names (including skill:) are verbatim.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandOption {
    pub name: String,
    pub description: Option<String>,
    pub source: CommandSource,
    pub source_info: Option<CommandSourceInfo>,
}
#[derive(Default)]
pub(super) struct Commands {
    pub options: Vec<CommandOption>,
    query: Option<String>,
    visible: Vec<usize>,
    dismissed: bool,
    cursor: usize,
    scroll: ScrollHandle,
    reveal: Rc<Cell<bool>>,
    pub bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    top_boundary: Pixels,
}
fn slash_query(draft: &str) -> Option<&str> {
    draft
        .strip_prefix('/')
        .filter(|q| !q.chars().any(|c| c.is_whitespace() || c == '/'))
}
fn fuzzy_score(value: &str, query: &str) -> Option<usize> {
    let candidate: Vec<_> = value.to_lowercase().encode_utf16().collect();
    let needle: Vec<_> = query.to_lowercase().encode_utf16().collect();
    let mut first = None;
    let mut previous = None;
    let mut gaps = 0;
    let mut qi = 0;
    for (i, c) in candidate.iter().enumerate() {
        if qi == needle.len() {
            break;
        }
        if *c != needle[qi] {
            continue;
        }
        first.get_or_insert(i);
        if let Some(p) = previous {
            gaps += i - p - 1;
        }
        previous = Some(i);
        qi += 1;
    }
    (qi == needle.len()).then(|| first.unwrap_or(0) * 8 + gaps * 2 + candidate.len() - needle.len())
}
impl Commands {
    fn sync(&mut self, draft: &str) {
        let query = slash_query(draft);
        if query != self.query.as_deref() {
            self.query = query.map(str::to_owned);
            self.refilter();
            self.cursor = 0;
            self.reveal.set(true);
            if self.query.is_none() {
                self.dismissed = false;
            }
        }
    }
    fn indices(&self) -> &[usize] {
        &self.visible
    }
    fn refilter(&mut self) {
        let Some(query) = &self.query else {
            self.visible.clear();
            return;
        };
        let mut matches: Vec<_> = self
            .options
            .iter()
            .enumerate()
            .filter_map(|(i, command)| fuzzy_score(&command.name, query).map(|s| (s, i)))
            .collect();
        if !query.is_empty() {
            matches.sort_by_key(|&(score, index)| (score, index));
        }
        self.visible = matches.into_iter().map(|(_, i)| i).collect();
    }
}
impl Composer {
    pub fn set_commands(&mut self, commands: Vec<CommandOption>, cx: &mut Context<Self>) {
        if self.commands.options == commands {
            return;
        }
        self.commands.options = commands;
        self.commands.refilter();
        self.commands.cursor = 0;
        self.commands.reveal.set(true);
        self.sync_commands(cx);
        cx.notify();
    }
    /// Window-coordinate bottom of the header, matching Vue's headerElement boundary.
    pub fn set_command_top_boundary(&mut self, boundary: Pixels, cx: &mut Context<Self>) {
        if self.commands.top_boundary != boundary {
            self.commands.top_boundary = boundary;
            cx.notify();
        }
    }
    pub fn command_menu_active(&self) -> bool {
        !self.disabled && !self.commands.dismissed && !self.commands.indices().is_empty()
    }
    pub fn active_command(&self) -> Option<&CommandOption> {
        self.command_menu_active()
            .then(|| self.commands.indices().get(self.commands.cursor).copied())
            .flatten()
            .map(|i| &self.commands.options[i])
    }
    pub(super) fn sync_commands(&mut self, cx: &gpui_kit::App) {
        self.commands.sync(self.editor.read(cx).value().as_ref());
    }
    pub(super) fn extension_command_draft(&self, cx: &gpui_kit::App) -> bool {
        let value = self.editor.read(cx).value();
        let Some(name) = value.trim().strip_prefix('/') else {
            return false;
        };
        let name = name.split(' ').next().unwrap_or("");
        !name.is_empty()
            && self
                .commands
                .options
                .iter()
                .any(|c| c.source == CommandSource::Extension && c.name == name)
    }
    fn select_command(
        &mut self,
        index: usize,
        complete: bool,
        follow_up: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_commands(cx);
        if !self.command_menu_active() {
            return;
        }
        let Some(command) = self.commands.options.get(index) else {
            return;
        };
        if !self.commands.indices().contains(&index) {
            return;
        }
        let draft = format!("/{}{}", command.name, if complete { " " } else { "" });
        self.editor.update(cx, |editor, cx| {
            let length = editor.value().encode_utf16().count();
            editor.replace_text_in_range(Some(0..length), &draft, window, cx);
        });
        self.sync_commands(cx);
        self.commands.cursor = 0;
        if !complete {
            self.send(follow_up, window, cx);
        }
        cx.notify();
    }
    pub(super) fn command_keys(
        &mut self,
        event: &gpui_kit::KeystrokeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(
            event.keystroke.key.as_str(),
            "up" | "down" | "tab" | "enter" | "escape"
        ) {
            return;
        }
        if !self.editor.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        if self
            .editor
            .update(cx, |e, cx| e.marked_text_range(window, cx).is_some())
        {
            return;
        }
        self.sync_commands(cx);
        if !self.command_menu_active() {
            return;
        }
        let key = &event.keystroke;
        let modifiers = key.modifiers;
        match key.key.as_str() {
            "escape" => self.commands.dismissed = true,
            "up" | "down" => {
                let count = self.commands.indices().len();
                self.commands.cursor = if key.key == "down" {
                    (self.commands.cursor + 1).min(count - 1)
                } else {
                    self.commands.cursor.saturating_sub(1)
                };
                self.commands.reveal.set(true);
            }
            "tab"
                if !modifiers.shift
                    && !modifiers.control
                    && !modifiers.platform
                    && !modifiers.alt =>
            {
                let index = self.commands.indices()[self.commands.cursor];
                self.select_command(index, true, false, window, cx);
            }
            "enter" if !modifiers.shift && !modifiers.alt => {
                let index = self.commands.indices()[self.commands.cursor];
                self.select_command(
                    index,
                    false,
                    modifiers.platform || modifiers.control,
                    window,
                    cx,
                );
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
    pub(super) fn install_command_keys(&mut self, window: &Window, cx: &mut Context<Self>) {
        let attached_window = window.window_handle().window_id();
        let owner = cx.entity().downgrade();
        self._command_keys = Some(cx.intercept_keystrokes(move |event, window, cx| {
            if window.window_handle().window_id() == attached_window {
                _ = owner.update(cx, |this, cx| this.command_keys(event, window, cx));
            }
        }));
    }
    pub(super) fn editor_event(
        &mut self,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_commands(cx);
        if let InputEvent::PressEnter {
            shift: false,
            secondary,
        } = event
        {
            if self.command_menu_active() {
                let index = self.commands.indices()[self.commands.cursor];
                self.select_command(index, false, *secondary, window, cx);
            } else {
                self.send(*secondary, window, cx);
            }
        }
        cx.notify();
    }
    pub(super) fn command_popup(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        if !self.command_menu_active() {
            return None;
        }
        let bounds = self.editor_bounds.get()?;
        let visible = self.commands.indices();
        let content_height = 18.
            + visible
                .iter()
                .map(|&i| {
                    if self.commands.options[i]
                        .description
                        .as_ref()
                        .is_some_and(|s| !s.is_empty())
                    {
                        40.5
                    } else {
                        24.
                    }
                })
                .sum::<f32>();
        let viewport = window.viewport_size();
        let desired = px(content_height.min(300.));
        let above = (bounds.top() - px(5.) - self.commands.top_boundary).max(px(0.));
        let below_top = bounds.top() + px((4_f32 + 13. * 1.45 + 5.).round());
        let below = (viewport.height - px(5.) - below_top).max(px(0.));
        let place_below = desired > above && below > above;
        // Keep the Vue minimum where possible, but never overflow a tiny native viewport.
        let height = desired
            .min(if place_below { below } else { above })
            .max(px(96.))
            .min(if place_below { below } else { above });
        let t = support::tokens(self.dark);
        let popup_bounds = self.commands.bounds.clone();
        let mut menu = div()
            .id("command-menu")
            .test_support()
            .role(gpui_kit::accesskit::Role::ListBox)
            .aria_label("Commands")
            .w(px(540.).min((bounds.size.width - px(8.)).max(px(0.))))
            .max_h(height)
            .min_h_0()
            .p(px(8.))
            .border_1()
            .border_color(rgb(t.border))
            .rounded(px(8.))
            .bg(rgb(t.panel_raised))
            .font_family(super::super::font::family())
            .map(|mut menu| {
                menu.text_style().fractional_line_height = Some(true);
                menu
            })
            .shadow(vec![
                BoxShadow::new(
                    px(0.),
                    px(8.),
                    rgba(if self.dark { 0x00000073 } else { 0x0000001f }).into(),
                )
                .blur_radius(px(20.)),
            ])
            .overflow_y_scroll()
            .track_scroll(&self.commands.scroll)
            .flex()
            .flex_col()
            .on_prepaint(move |bounds, _, _| popup_bounds.set(Some(bounds)))
            .occlude()
            .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                cx.stop_propagation()
            });
        for (position, &index) in visible.iter().enumerate() {
            let command = &self.commands.options[index];
            let identity = command.clone();
            let active = position == self.commands.cursor;
            let pending = self.commands.reveal.clone();
            let scroll = self.commands.scroll.clone();
            let mut copy = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(support::mono())
                        .text_size(px(11.))
                        // Vue's monospace command name inherits normal leading, not 1.45.
                        .line_height(px(13.))
                        .child(format!("/{}", command.name)),
                );
            if let Some(description) = command.description.as_ref().filter(|s| !s.is_empty()) {
                copy = copy.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(10.))
                        .line_height(gpui_kit::relative(1.45))
                        .text_color(rgb(t.muted))
                        .child(description.clone()),
                );
            }
            menu = menu.child(
                div()
                    .id(format!("command-option-{position}"))
                    .test_support()
                    .role(gpui_kit::accesskit::Role::ListBoxOption)
                    .aria_label(format!(
                        "/{} — {}{}",
                        command.name,
                        command.source.as_str(),
                        command
                            .description
                            .as_ref()
                            .map(|text| format!(": {text}"))
                            .unwrap_or_default()
                    ))
                    .aria_selected(active)
                    .when(active, |d| d.aria_active_descendant())
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .relative()
                    .rounded(px(6.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .pt(px(5.))
                    .pb(px(6.))
                    .flex()
                    .items_baseline()
                    .gap(px(10.))
                    .text_color(rgb(t.text))
                    .when(active, |d| {
                        d.bg(if self.dark {
                            rgb(0x475266).opacity(0.2)
                        } else {
                            rgb(0x6b7d8f).opacity(0.1)
                        })
                        .child(
                            div()
                                .absolute()
                                .left(px(0.))
                                .top(px(3.))
                                .bottom(px(3.))
                                .w(px(2.))
                                .rounded_tr(px(1.))
                                .rounded_br(px(1.))
                                .bg(rgb(t.accent)),
                        )
                    })
                    .on_prepaint(move |bounds, window, _| {
                        if !active || !pending.get() {
                            return;
                        }
                        let viewport = scroll.bounds();
                        let delta = if bounds.top() < viewport.top() + px(9.) {
                            viewport.top() + px(9.) - bounds.top()
                        } else if bounds.bottom() > viewport.bottom() - px(9.) {
                            viewport.bottom() - px(9.) - bounds.bottom()
                        } else {
                            px(0.)
                        };
                        if delta == px(0.) || bounds.size.height > viewport.size.height - px(18.) {
                            pending.set(false);
                        } else {
                            let mut offset = scroll.offset();
                            offset.y += delta;
                            scroll.set_offset(offset);
                            window.request_animation_frame();
                        }
                    })
                    .on_mouse_move(cx.listener(move |s, _, _, cx| {
                        if s.commands.cursor != position {
                            s.commands.cursor = position;
                            s.commands.reveal.set(false);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |s, _, window, cx| {
                        if s.commands.options.get(index) == Some(&identity) {
                            s.select_command(index, false, false, window, cx);
                        }
                    }))
                    .child(copy)
                    .child(
                        div()
                            .flex_shrink_0()
                            .self_end()
                            .text_size(px(10.))
                            .line_height(px(12.))
                            .text_color(rgb(t.faint))
                            .child(command.source.as_str()),
                    ),
            );
        }
        Some(
            gpui_kit::deferred(
                Positioner::corner(
                    if place_below {
                        Anchor::TopLeft
                    } else {
                        Anchor::BottomLeft
                    },
                    gpui_kit::point(
                        bounds.left() + px(4.),
                        if place_below {
                            below_top
                        } else {
                            bounds.top() - px(5.)
                        },
                    ),
                )
                .margin(px(0.))
                .child(menu),
            )
            .with_priority(100)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests;

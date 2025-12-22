use crate::types::{DwarfInfo, DwarfSymbol, DwarfTag};
use crate::utils::format_size;
use gpui::{prelude::*, *};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollbarAxis;
use gpui_component::{ActiveTheme, StyledExt};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct DwarfSymbolSelectEvent {
    pub symbol: DwarfSymbol,
}

impl EventEmitter<DwarfSymbolSelectEvent> for DwarfTreePanel {}

/// A flattened node for display in the tree
#[derive(Clone)]
struct FlatNode {
    symbol: Arc<DwarfSymbol>,
    depth: usize,
}

pub struct DwarfTreePanel {
    dwarf_info: Arc<DwarfInfo>,
    expanded_ids: HashSet<usize>,
    selected_id: Option<usize>,
    search_input: Entity<InputState>,
    search_query: String,
    focus_handle: FocusHandle,
    /// Cached flat list of visible nodes - rebuilt only when needed
    cached_nodes: Vec<FlatNode>,
    /// Whether the cache needs to be rebuilt
    cache_dirty: bool,
}

impl Focusable for DwarfTreePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl DwarfTreePanel {
    pub fn new(dwarf_info: DwarfInfo, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search symbols..."));

        cx.subscribe(&search_input, Self::on_search_input).detach();

        // Auto-expand the first compile unit on startup
        let mut expanded_ids = HashSet::new();
        if let Some(first_cu) = dwarf_info.compile_units.first() {
            expanded_ids.insert(first_cu.id);
        }

        let dwarf_info = Arc::new(dwarf_info);

        Self {
            dwarf_info,
            expanded_ids,
            selected_id: None,
            search_input,
            search_query: String::new(),
            focus_handle: cx.focus_handle(),
            cached_nodes: Vec::new(),
            cache_dirty: true,
        }
    }

    fn on_search_input(
        &mut self,
        input: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if let InputEvent::Change = event {
            self.search_query = input.read(cx).text().to_string();
            self.cache_dirty = true;
            cx.notify();
        }
    }

    fn toggle_expanded(&mut self, id: usize, cx: &mut Context<Self>) {
        if self.expanded_ids.contains(&id) {
            self.expanded_ids.remove(&id);
        } else {
            self.expanded_ids.insert(id);
        }
        self.cache_dirty = true;
        cx.notify();
    }

    fn select_symbol(
        &mut self,
        symbol: &DwarfSymbol,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_id = Some(symbol.id);
        cx.emit(DwarfSymbolSelectEvent {
            symbol: symbol.clone(),
        });
        cx.notify();
    }

    fn rebuild_cache(&mut self) {
        self.cached_nodes.clear();

        // Clone the Arc to avoid borrow issues
        let dwarf_info = Arc::clone(&self.dwarf_info);
        let expanded_ids = self.expanded_ids.clone();
        let search_query = self.search_query.clone();

        if search_query.is_empty() {
            // No search - just show expanded nodes
            for cu in &dwarf_info.compile_units {
                Self::collect_expanded_nodes_static(cu, 0, &expanded_ids, &mut self.cached_nodes);
            }
        } else {
            // Search mode - show matching nodes (limited to avoid lag)
            let query = search_query.to_lowercase();
            let mut count = 0;
            const MAX_SEARCH_RESULTS: usize = 200;

            for cu in &dwarf_info.compile_units {
                Self::collect_matching_nodes_static(
                    cu,
                    0,
                    &query,
                    &mut count,
                    MAX_SEARCH_RESULTS,
                    &mut self.cached_nodes,
                );
                if count >= MAX_SEARCH_RESULTS {
                    break;
                }
            }
        }

        self.cache_dirty = false;
    }

    fn collect_expanded_nodes_static(
        symbol: &DwarfSymbol,
        depth: usize,
        expanded_ids: &HashSet<usize>,
        nodes: &mut Vec<FlatNode>,
    ) {
        nodes.push(FlatNode {
            symbol: Arc::new(symbol.clone()),
            depth,
        });

        if expanded_ids.contains(&symbol.id) {
            // Sort children: named first, then anonymous
            let mut sorted_children: Vec<_> = symbol.children.iter().collect();
            sorted_children.sort_by(|a, b| {
                let a_anon = a.name.starts_with('<') && a.name.ends_with('>');
                let b_anon = b.name.starts_with('<') && b.name.ends_with('>');
                match (a_anon, b_anon) {
                    (true, false) => std::cmp::Ordering::Greater,
                    (false, true) => std::cmp::Ordering::Less,
                    _ => a.name.cmp(&b.name),
                }
            });

            for child in sorted_children {
                Self::collect_expanded_nodes_static(child, depth + 1, expanded_ids, nodes);
            }
        }
    }

    fn collect_matching_nodes_static(
        symbol: &DwarfSymbol,
        depth: usize,
        query: &str,
        count: &mut usize,
        max: usize,
        nodes: &mut Vec<FlatNode>,
    ) {
        if *count >= max {
            return;
        }

        if symbol.name.to_lowercase().contains(query) {
            nodes.push(FlatNode {
                symbol: Arc::new(symbol.clone()),
                depth,
            });
            *count += 1;
        }

        // Always search children
        for child in &symbol.children {
            Self::collect_matching_nodes_static(child, depth + 1, query, count, max, nodes);
            if *count >= max {
                return;
            }
        }
    }

    fn render_tree_node(&self, node: &FlatNode, cx: &App) -> Stateful<Div> {
        let symbol = &node.symbol;
        let depth = node.depth;
        let is_expanded = self.expanded_ids.contains(&symbol.id);
        let is_selected = self.selected_id == Some(symbol.id);
        let has_children = !symbol.children.is_empty();
        let indent = depth * 16;

        let tag_color = match symbol.tag {
            DwarfTag::CompileUnit => rgb(0x61afef),
            DwarfTag::Subprogram => rgb(0xc678dd),
            DwarfTag::Variable => rgb(0xe5c07b),
            DwarfTag::FormalParameter => rgb(0xd19a66),
            DwarfTag::StructureType => rgb(0x98c379),
            DwarfTag::UnionType => rgb(0x98c379),
            DwarfTag::EnumerationType => rgb(0x56b6c2),
            DwarfTag::Member => rgb(0xabb2bf),
            DwarfTag::Typedef => rgb(0xe06c75),
            DwarfTag::Namespace => rgb(0x61afef),
            DwarfTag::LexicalBlock => rgb(0x5c6370),
            DwarfTag::InlinedSubroutine => rgb(0xc678dd),
            DwarfTag::Other(_) => rgb(0xabb2bf),
        };

        let mut row = div()
            .id(ElementId::Name(format!("dwarf-node-{}", symbol.id).into()))
            .flex()
            .items_center()
            .w_full()
            .pl(px(indent as f32 + 8.0))
            .pr_2()
            .py_1()
            .gap_1()
            .cursor_pointer()
            .rounded_sm()
            .when(is_selected, |d| {
                d.bg(cx.theme().accent)
                    .text_color(cx.theme().accent_foreground)
            })
            .when(!is_selected, |d| d.hover(|d| d.bg(cx.theme().list_hover)));

        // Expand/collapse chevron
        if has_children {
            let chevron = if is_expanded { "▼" } else { "▶" };
            row = row.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .w(px(12.0))
                    .child(chevron),
            );
        } else {
            row = row.child(div().w(px(12.0)));
        }

        // Tag icon
        let icon = symbol.tag.icon().to_string();
        row = row.child(
            div()
                .text_xs()
                .text_color(tag_color)
                .w(px(20.0))
                .child(icon),
        );

        // Symbol name (truncated)
        let display_name = if symbol.name.len() > 50 {
            format!("{}...", &symbol.name[..47])
        } else {
            symbol.name.clone()
        };

        row = row.child(
            div()
                .flex_1()
                .text_sm()
                .overflow_hidden()
                .text_ellipsis()
                .child(display_name),
        );

        // Address badge
        if let Some(addr) = symbol.address {
            row = row.child(
                div()
                    .text_xs()
                    .font_family("monospace")
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("0x{:08x}", addr)),
            );
        }

        // Size badge
        if let Some(size) = symbol.size {
            if size > 0 {
                row = row.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .ml_1()
                        .child(format_size(size)),
                );
            }
        }

        row
    }
}

impl Render for DwarfTreePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Rebuild cache if needed
        if self.cache_dirty {
            self.rebuild_cache();
        }

        // Limit rendered nodes for performance
        const MAX_RENDERED: usize = 500;
        let nodes_to_render = if self.cached_nodes.len() > MAX_RENDERED {
            &self.cached_nodes[..MAX_RENDERED]
        } else {
            &self.cached_nodes[..]
        };

        let truncated = self.cached_nodes.len() > MAX_RENDERED;

        div()
            .id("dwarf_tree_panel")
            .flex()
            .flex_col()
            .size_full()
            .child(
                // Header
                div()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(cx.theme().foreground)
                                    .child("DWARF Symbols"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{} symbols", self.dwarf_info.total_symbols)),
                            ),
                    ),
            )
            .child(
                // Search input
                div()
                    .px_2()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(Input::new(&self.search_input)),
            )
            .child(
                // Tree content
                div().flex_1().overflow_hidden().child(
                    div()
                        .size_full()
                        .scrollable(ScrollbarAxis::Vertical)
                        .children(nodes_to_render.iter().map(|node| {
                            let symbol = (*node.symbol).clone();
                            let symbol_id = symbol.id;
                            let has_children = !symbol.children.is_empty();

                            self.render_tree_node(node, cx).on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |view, _event, window, cx| {
                                    if has_children {
                                        view.toggle_expanded(symbol_id, cx);
                                    }
                                    view.select_symbol(&symbol, window, cx);
                                }),
                            )
                        }))
                        .when(truncated, |d| {
                            d.child(
                                div()
                                    .px_3()
                                    .py_2()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "... and {} more (expand folders to see more)",
                                        self.cached_nodes.len() - MAX_RENDERED
                                    )),
                            )
                        }),
                ),
            )
    }
}

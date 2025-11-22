use crate::types::ElfSymbol;
use crate::utils::format_size;
use gpui::{prelude::*, *};
use gpui_component::table::{Column, ColumnSort, TableDelegate, TableState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortColumn {
    Name,
    Address,
    Size,
}

pub struct SymbolsTableDelegate {
    pub symbols: Vec<ElfSymbol>,
    columns: Vec<Column>,
    sorted_column: Option<SortColumn>,
    sort_direction: ColumnSort,
}

impl SymbolsTableDelegate {
    pub fn new(mut symbols: Vec<ElfSymbol>) -> Self {
        // Default sort by address
        symbols.sort_by_key(|s| s.size);
        symbols = symbols
            .into_iter()
            .rev()
            .map(|mut s| {
                for lang in [
                    gimli::DW_LANG_Rust,
                    gimli::DW_LANG_C_plus_plus,
                    gimli::DW_LANG_C_plus_plus_03,
                    gimli::DW_LANG_C_plus_plus_11,
                    gimli::DW_LANG_C_plus_plus_14,
                ] {
                    if let Some(demangle) = addr2line::demangle(&s.name, lang) {
                        s.name = demangle;
                        break;
                    }
                }
                s
            })
            .collect();
        let columns = vec![
            Column::new("name", "Symbol Name")
                .width(px(400.0))
                .sortable(),
            Column::new("address", "Address")
                .width(px(160.0))
                .text_right()
                .sortable(),
            Column::new("size", "Size")
                .width(px(120.0))
                .text_right()
                .sortable(),
        ];

        Self {
            symbols,
            columns,
            sorted_column: None,
            sort_direction: ColumnSort::Default,
        }
    }

    fn sort_symbols(&mut self) {
        if let Some(col) = self.sorted_column {
            match self.sort_direction {
                ColumnSort::Ascending => match col {
                    SortColumn::Name => self.symbols.sort_by(|a, b| a.name.cmp(&b.name)),
                    SortColumn::Address => self.symbols.sort_by_key(|s| s.address),
                    SortColumn::Size => self.symbols.sort_by_key(|s| s.size),
                },
                ColumnSort::Descending => match col {
                    SortColumn::Name => self.symbols.sort_by(|a, b| b.name.cmp(&a.name)),
                    SortColumn::Address => {
                        self.symbols.sort_by_key(|s| std::cmp::Reverse(s.address))
                    }
                    SortColumn::Size => self.symbols.sort_by_key(|s| std::cmp::Reverse(s.size)),
                },
                ColumnSort::Default => {
                    // Default sort by address ascending
                    self.symbols.sort_by_key(|s| s.address);
                }
            }
        } else {
            // Default sort by address
            self.symbols.sort_by_key(|s| s.address);
        }
    }
}

impl TableDelegate for SymbolsTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.symbols.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> &Column {
        &self.columns[col_ix]
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        // Determine which column was clicked
        self.sorted_column = match col_ix {
            0 => Some(SortColumn::Name),
            1 => Some(SortColumn::Address),
            2 => Some(SortColumn::Size),
            _ => None,
        };

        self.sort_direction = sort;
        self.sort_symbols();

        // Notify the table to refresh with the new sorted data
        cx.notify();
    }

    fn render_td(
        &self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut App,
    ) -> impl IntoElement {
        let symbol = &self.symbols[row_ix];

        let content = match col_ix {
            0 => symbol.name.clone(),
            1 => format!("0x{:016x}", symbol.address),
            2 => format_size(symbol.size),
            _ => String::new(),
        };

        div()
            .text_sm()
            .text_color(rgb(0xcccccc))
            .when(col_ix == 1, |div| div.font_family("monospace"))
            .child(content)
    }
}

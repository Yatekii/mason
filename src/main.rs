mod components;
mod parser;
mod types;
mod utils;

use anyhow::{Context as AnyhowContext, Result};
use clap::Parser;
use components::MemoryView;
use gpui::*;
use gpui_component::theme::{Theme, ThemeRegistry};
use gpui_component::{Root, TitleBar};
use gpui_component_assets::Assets;
use parser::{
    load_memory_layout_from_probe_rs, parse_defmt_info, parse_dwarf_info, parse_elf_segments,
    parse_elf_symbols, parse_rtt_info,
};
use std::env;
use std::path::PathBuf;

/// A DWARF debug symbol browser for ELF files
#[derive(Parser, Debug)]
#[command(name = "mason", version, about)]
struct Args {
    /// Path to the ELF file to analyze
    elf_file: PathBuf,

    /// Target chip for memory layout (e.g., STM32F407VGTx)
    #[arg(short, long)]
    target: Option<String>,
}

actions!(mason, [Quit]);

fn main() -> Result<()> {
    let args = Args::parse();

    let elf_path = args.elf_file;

    if !elf_path.exists() {
        eprintln!("Error: File '{}' does not exist", elf_path.display());
        std::process::exit(1);
    }

    let current_target = args.target;

    // Load memory regions if target is specified
    let memory_regions = if let Some(ref target) = current_target {
        load_memory_layout_from_probe_rs(target).context("Failed to load target from probe-rs")?
    } else {
        Vec::new()
    };

    // Always parse ELF segments (conflict detection only if we have memory regions)
    let segments = parse_elf_segments(
        &elf_path,
        if memory_regions.is_empty() {
            None
        } else {
            Some(&memory_regions)
        },
    )
    .context("Failed to parse ELF segments")?;

    if segments.is_empty() {
        eprintln!("Warning: No loadable segments found in ELF file");
    }

    let symbols = parse_elf_symbols(&elf_path).context("Failed to parse ELF symbols")?;
    eprintln!("Found {} symbols in ELF file", symbols.len());

    let defmt_info = parse_defmt_info(&elf_path).context("Failed to parse defmt info")?;
    let rtt_info = parse_rtt_info(&elf_path).context("Failed to parse RTT info")?;
    let dwarf_info = parse_dwarf_info(&elf_path).unwrap_or_else(|e| {
        eprintln!("Warning: Failed to parse DWARF info: {}", e);
        types::DwarfInfo::default()
    });
    eprintln!(
        "Found {} DWARF compile units with {} total symbols",
        dwarf_info.compile_units.len(),
        dwarf_info.total_symbols
    );

    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            // Initialize gpui-component before using any components
            gpui_component::init(cx);

            // Load custom themes from themes directory
            let themes_dir = env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("themes");

            if themes_dir.exists() {
                let _ = ThemeRegistry::watch_dir(themes_dir, cx, |cx| {
                    // Set Twilight as the default theme after themes are loaded
                    let theme_registry = ThemeRegistry::global(cx);
                    let twilight_name: SharedString = "Twilight".into();
                    if let Some(twilight_theme) = theme_registry.themes().get(&twilight_name) {
                        let twilight_theme = twilight_theme.clone();
                        let theme_mode = twilight_theme.mode;

                        let theme = Theme::global_mut(cx);
                        theme.dark_theme = twilight_theme;
                        Theme::change(theme_mode, None, cx);
                    }
                });
            }

            let bounds = Bounds::centered(None, size(px(1600.0), px(900.0)), cx);

            // Set up keyboard bindings
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);

            // Handle quit action
            cx.on_action(|_: &Quit, cx| cx.quit());

            let window_options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitleBar::title_bar_options()),
                ..Default::default()
            };

            cx.spawn(async move |cx| {
                let window = cx.open_window(window_options, |window, cx| {
                    let view = cx.new(|cx| {
                        MemoryView::new(
                            segments.clone(),
                            memory_regions.clone(),
                            symbols.clone(),
                            defmt_info.clone(),
                            rtt_info.clone(),
                            dwarf_info.clone(),
                            current_target.clone(),
                            elf_path.clone(),
                            window,
                            cx,
                        )
                    });
                    // Wrap in Root component for gpui-component
                    cx.new(|cx| Root::new(view, window, cx))
                })?;

                // Get the root view entity and observe when it's released (window closed)
                let root_view = window.update(cx, |_, _, cx| cx.entity())?;
                cx.update(|cx| {
                    cx.observe_release(&root_view, |_, cx| cx.quit()).detach();
                })?;

                cx.update(|cx| cx.activate(true))?;

                Ok::<_, anyhow::Error>(())
            })
            .detach();
        });

    Ok(())
}

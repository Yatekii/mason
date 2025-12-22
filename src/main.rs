mod components;
mod parser;
mod types;
mod utils;

use anyhow::{Context as AnyhowContext, Result};
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

actions!(mason, [Quit]);

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: {} <elf-file> --target <probe-rs-target>", args[0]);
        eprintln!();
        eprintln!("Example:");
        eprintln!("  {} firmware.elf --target STM32F407VGTx", args[0]);
        eprintln!();
        eprintln!("To list available probe-rs targets, run:");
        eprintln!("  probe-rs chip list");
        std::process::exit(1);
    }

    let elf_path = PathBuf::from(&args[1]);

    if !elf_path.exists() {
        eprintln!("Error: File '{}' does not exist", elf_path.display());
        std::process::exit(1);
    }

    // Parse target argument
    if args[2] != "--target" && args[2] != "-t" {
        eprintln!("Error: Expected --target flag");
        eprintln!("Usage: {} <elf-file> --target <probe-rs-target>", args[0]);
        std::process::exit(1);
    }

    if args.len() < 4 {
        eprintln!("Error: --target requires a target name");
        std::process::exit(1);
    }

    let memory_regions = load_memory_layout_from_probe_rs(&args[3])
        .context("Failed to load target from probe-rs")?;

    let segments =
        parse_elf_segments(&elf_path, &memory_regions).context("Failed to parse ELF segments")?;

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
                            args[3].clone(),
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

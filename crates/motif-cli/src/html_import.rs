//! Import explicit, local HTML and CSS inputs without executing or fetching resources.
use super::Args;
use std::io::Write;

pub(super) fn run(args: &Args, html: &str) -> Result<(), String> {
    let css = args.css.as_ref().map(|p| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))).transpose()?.unwrap_or_default();
    let options = motif_html::ImportOptions { browser: args.browser.unwrap_or_default() };
    let imported = motif_html::import_with_options(html, &css, options)?;
    if let Some(out) = &args.out {
        if [&args.scene, args.css.as_ref().unwrap_or(&args.scene)]
            .iter()
            .any(|input| out.canonicalize().ok().zip(input.canonicalize().ok()).is_some_and(|(a, b)| a == b))
        {
            return Err("import-html: файл MSC не должен заменять входной HTML или CSS".into());
        }
        if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(out, &imported.source).map_err(|e| format!("{}: {e}", out.display()))?;
        eprintln!("{} — импортировано элементов: {}", out.display(), imported.elements);
    } else {
        std::io::stdout().lock().write_all(imported.source.as_bytes()).map_err(|e| e.to_string())?;
    }
    for warning in imported.warnings {
        eprintln!("Предупреждение: {warning}");
    }
    Ok(())
}

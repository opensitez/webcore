//! Browser chrome dialogs for the embedded Webcore shell.

use std::path::PathBuf;

pub fn alert(message: &str) {
    rfd::MessageDialog::new()
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

pub fn confirm(message: &str) -> bool {
    matches!(
        rfd::MessageDialog::new()
            .set_description(message)
            .set_buttons(rfd::MessageButtons::OkCancel)
            .show(),
        rfd::MessageDialogResult::Ok
    )
}

fn file_dialog(title: &str, filters: &[(String, Vec<String>)], directory: &str) -> rfd::FileDialog {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    if !directory.is_empty() {
        dialog = dialog.set_directory(directory);
    }
    for (name, extensions) in filters {
        let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(name, &extensions);
    }
    dialog
}

pub fn open_file(
    title: &str,
    filters: &[(String, Vec<String>)],
    directory: &str,
    multiple: bool,
) -> Vec<PathBuf> {
    let dialog = file_dialog(title, filters, directory);
    if multiple {
        dialog.pick_files().unwrap_or_default()
    } else {
        dialog.pick_file().into_iter().collect()
    }
}

pub fn save_file(
    title: &str,
    filters: &[(String, Vec<String>)],
    directory: &str,
    suggested: &str,
) -> Option<PathBuf> {
    let mut dialog = file_dialog(title, filters, directory);
    if !suggested.is_empty() {
        dialog = dialog.set_file_name(suggested);
    }
    dialog.save_file()
}

pub fn pick_directory(title: &str, directory: &str) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    if !directory.is_empty() {
        dialog = dialog.set_directory(directory);
    }
    dialog.pick_folder()
}

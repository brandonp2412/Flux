use std::ffi::{CString, c_char, c_int};

#[repr(C)]
struct GtkWidget {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn gtk_init();
    fn gtk_window_new() -> *mut GtkWidget;
    fn gtk_window_set_title(window: *mut GtkWidget, title: *const c_char);
    fn gtk_window_set_default_size(window: *mut GtkWidget, width: c_int, height: c_int);
    fn gtk_window_set_child(window: *mut GtkWidget, child: *mut GtkWidget);
    fn gtk_grid_new() -> *mut GtkWidget;
    fn gtk_grid_set_column_spacing(grid: *mut GtkWidget, spacing: u32);
    fn gtk_grid_set_row_spacing(grid: *mut GtkWidget, spacing: u32);
    fn gtk_widget_set_margin_top(widget: *mut GtkWidget, margin: c_int);
    fn gtk_widget_set_margin_bottom(widget: *mut GtkWidget, margin: c_int);
    fn gtk_widget_set_margin_start(widget: *mut GtkWidget, margin: c_int);
    fn gtk_widget_set_margin_end(widget: *mut GtkWidget, margin: c_int);
    fn gtk_label_new(text: *const c_char) -> *mut GtkWidget;
    fn gtk_button_new_with_label(text: *const c_char) -> *mut GtkWidget;
    fn gtk_grid_attach(
        grid: *mut GtkWidget,
        child: *mut GtkWidget,
        column: c_int,
        row: c_int,
        width: c_int,
        height: c_int,
    );
}

fn text(value: &str) -> CString {
    CString::new(value).expect("benchmark labels contain no NUL bytes")
}

fn main() {
    unsafe {
        gtk_init();

        let window = gtk_window_new();
        let window_title = text("Flux Native UI Benchmark");
        gtk_window_set_title(window, window_title.as_ptr());
        gtk_window_set_default_size(window, 420, 260);

        let grid = gtk_grid_new();
        gtk_grid_set_column_spacing(grid, 12);
        gtk_grid_set_row_spacing(grid, 12);
        gtk_widget_set_margin_top(grid, 24);
        gtk_widget_set_margin_bottom(grid, 24);
        gtk_widget_set_margin_start(grid, 24);
        gtk_widget_set_margin_end(grid, 24);
        gtk_window_set_child(window, grid);

        let title_text = text("Flux Native UI");
        let status_text = text("Ready");
        let action_text = text("Toggle");
        let title = gtk_label_new(title_text.as_ptr());
        let status = gtk_label_new(status_text.as_ptr());
        let action = gtk_button_new_with_label(action_text.as_ptr());
        gtk_grid_attach(grid, title, 0, 0, 1, 1);
        gtk_grid_attach(grid, status, 0, 1, 1, 1);
        gtk_grid_attach(grid, action, 0, 2, 1, 1);
    }
}

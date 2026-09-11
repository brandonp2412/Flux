use std::ffi::{CString, c_char, c_int, c_uint, c_void};
use std::ptr;

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
    fn gtk_window_present(window: *mut GtkWidget);
    fn gtk_grid_new() -> *mut GtkWidget;
    fn gtk_grid_set_column_spacing(grid: *mut GtkWidget, spacing: u32);
    fn gtk_grid_set_row_spacing(grid: *mut GtkWidget, spacing: u32);
    fn gtk_widget_set_margin_top(widget: *mut GtkWidget, margin: c_int);
    fn gtk_widget_set_margin_bottom(widget: *mut GtkWidget, margin: c_int);
    fn gtk_widget_set_margin_start(widget: *mut GtkWidget, margin: c_int);
    fn gtk_widget_set_margin_end(widget: *mut GtkWidget, margin: c_int);
    fn gtk_widget_set_visible(widget: *mut GtkWidget, visible: c_int);
    fn gtk_widget_get_visible(widget: *mut GtkWidget) -> c_int;
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
    fn g_signal_connect_data(
        instance: *mut c_void,
        detailed_signal: *const c_char,
        handler: Option<unsafe extern "C" fn(*mut GtkWidget, *mut c_void)>,
        data: *mut c_void,
        destroy_data: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
        connect_flags: c_uint,
    ) -> u64;
    fn g_main_context_iteration(context: *mut c_void, may_block: c_int) -> c_int;
}

fn text(value: &str) -> CString {
    CString::new(value).expect("benchmark labels contain no NUL bytes")
}

unsafe extern "C" fn toggle_status(_button: *mut GtkWidget, data: *mut c_void) {
    let labels = data.cast::<*mut GtkWidget>();
    unsafe {
        gtk_widget_set_visible(*labels, 0);
        gtk_widget_set_visible(*labels.add(1), 1);
    }
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
        let ready_text = text("Ready");
        let toggled_text = text("Toggled");
        let action_text = text("Toggle");
        let clicked_signal = text("clicked");
        let title = gtk_label_new(title_text.as_ptr());
        let ready = gtk_label_new(ready_text.as_ptr());
        let toggled = gtk_label_new(toggled_text.as_ptr());
        let action = gtk_button_new_with_label(action_text.as_ptr());
        gtk_widget_set_visible(toggled, 0);
        gtk_grid_attach(grid, title, 0, 0, 1, 1);
        gtk_grid_attach(grid, ready, 0, 1, 1, 1);
        gtk_grid_attach(grid, toggled, 0, 2, 1, 1);
        gtk_grid_attach(grid, action, 0, 3, 1, 1);

        let mut status_labels = [ready, toggled];
        g_signal_connect_data(
            action.cast(),
            clicked_signal.as_ptr(),
            Some(toggle_status),
            status_labels.as_mut_ptr().cast(),
            None,
            0,
        );

        gtk_window_present(window);
        while gtk_widget_get_visible(window) != 0 {
            g_main_context_iteration(ptr::null_mut(), 1);
        }
    }
}

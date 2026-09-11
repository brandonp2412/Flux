#include <gtk/gtk.h>

int main(void) {
    gtk_init();

    GtkWidget *window = gtk_window_new();
    gtk_window_set_title(GTK_WINDOW(window), "Flux Native UI Benchmark");
    gtk_window_set_default_size(GTK_WINDOW(window), 420, 260);

    GtkWidget *grid = gtk_grid_new();
    gtk_grid_set_column_spacing(GTK_GRID(grid), 12);
    gtk_grid_set_row_spacing(GTK_GRID(grid), 12);
    gtk_widget_set_margin_top(grid, 24);
    gtk_widget_set_margin_bottom(grid, 24);
    gtk_widget_set_margin_start(grid, 24);
    gtk_widget_set_margin_end(grid, 24);
    gtk_window_set_child(GTK_WINDOW(window), grid);

    GtkWidget *title = gtk_label_new("Flux Native UI");
    GtkWidget *status = gtk_label_new("Ready");
    GtkWidget *action = gtk_button_new_with_label("Toggle");
    gtk_grid_attach(GTK_GRID(grid), title, 0, 0, 1, 1);
    gtk_grid_attach(GTK_GRID(grid), status, 0, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(grid), action, 0, 2, 1, 1);

    return 0;
}

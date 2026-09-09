fn pressed() -> void {
    print("pressed")
}

view Greeting(name: str, *, selectable: bool = false) {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: name
        selectable: selectable
}

view App {
    grid columns: 1fr 1fr
    grid rows: auto
    Greeting greeting at 1,1
        name: "Flux"
    Button action at 1,2
        text: "Continue"
        onPress: pressed
}

fn main() -> i64 { 42 }

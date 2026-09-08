struct Profile {
    name: str
    age: i64
}

struct User {
    profile: Profile
}

enum Event {
    Loaded(User)
    Empty
}

fn describe(event: Event) -> i64 {
    match event:
        Event.Loaded(User { profile: Profile { name, age: years } }):
            print(name)
            return years
        Event.Empty():
            return 0
}

fn main() -> i64 {
    let profile: Profile = Profile { name: "Ada", age: 42 }
    let user: User = User { profile: profile }
    print(describe(Event.Loaded(user)))
    return 0
}

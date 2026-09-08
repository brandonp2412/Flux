struct Profile {
    user: User
    active: bool
}

struct User {
    name: str
    age: i64
}

fn birthday(user: User) -> User {
    return User { name: user.name, age: user.age + 1 }
}

fn main() -> i64 {
    let profile: Profile = Profile { user: User { name: "Ada", age: 41 }, active: true }
    let disabled: Profile = Profile { ..profile, active: false }
    let older: User = birthday(disabled.user)
    let updated: Profile = Profile { ..disabled, user: older }
    let Profile { user: User { name, age: years }, active } = updated
    print(name)
    print(years)
    print(active)
    return 0
}

type UserId = i64
type Person = User

struct User {
    id: UserId
    name: str
}

fn idOf(user: Person) -> UserId {
    return user.id
}

fn main() -> i64 {
    let user: Person = User { id: 7, name: "Ada" }
    print(user.name)
    print(idOf(user))
    return 0
}

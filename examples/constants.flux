type Count = i64

const BASE: Count = 40
const ANSWER: Count = BASE + 2
const ENABLED: bool = ANSWER == 42
const LABEL: str = "Flux"

fn main() -> i64 {
    let folded: i64 = 2 * 20 + 2
    let comparison: bool = 10 > 3 && !false
    let input: i64 = 5
    let dynamic: i64 = input + 2
    print(LABEL)
    if ENABLED:
        print(ANSWER)
    print(folded)
    print(comparison)
    print(dynamic)
    return 0
}

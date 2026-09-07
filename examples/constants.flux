type Count = i64

const BASE: Count = 40
const ANSWER: Count = BASE + 2
const ENABLED: bool = ANSWER == 42
const LABEL: str = "Flux"

fn main() -> i64 {
    print(LABEL)
    if ENABLED:
        print(ANSWER)
    return 0
}

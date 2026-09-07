fn load(path: str) -> (str, error) {
    if path == "":
        return "", error("path is required")
    return "configuration loaded", nil
}

fn main() -> i64 {
    let data: str, err: error = load("settings.flux")
    if err != nil:
        print(err)
    print(data)

    let missing: str, missing_err: error = load("")
    if missing_err != nil:
        print(missing_err)
    print(missing)
    return 0
}

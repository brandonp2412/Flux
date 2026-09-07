fn load(path: str) -> (str, error) {
    if path == "":
        return "", error("path is required")
    return "configuration loaded", nil
}

fn load_config(path: str) -> (str, error) {
    let data: str, err: error = load(path) else return
    return data, nil
}

fn main() -> i64 {
    let data: str, err: error = load_config("settings.flux")
    if err != nil:
        print(err)
    print(data)

    let missing: str, missing_err: error = load_config("")
    if missing_err != nil:
        print(missing_err)
    print(missing)
    return 0
}

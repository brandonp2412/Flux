fn load(path: str) -> (str, error) {
    if path == "":
        return "", error("path is required")
    return "configuration loaded", nil
}

fn loadConfig(path: str) -> (str, error) {
    let data: str, err: error = load(path) else return
    return data, nil
}

fn main() -> i64 {
    let data: str, err: error = loadConfig("settings.flux")
    if err != nil:
        print(err)
    print(data)

    let missing: str, missingErr: error = loadConfig("")
    if missingErr != nil:
        print(missingErr)
    print(missing)
    return 0
}

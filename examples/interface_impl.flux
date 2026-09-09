interface Storage {
    fn load(path: str) -> (str, error)
    fn save(path: str, data: str, *, durable: bool) -> error
}

struct FileStorage {
    root: str
}

impl Storage for FileStorage {
    load: fileLoad
    save: fileSave
}

fn fileLoad(storage: FileStorage, path: str) -> (str, error) {
    print(storage.root)
    print(path)
    return "loaded", nil
}

fn fileSave(storage: FileStorage, path: str, data: str, *, durable: bool) -> error {
    print(storage.root)
    print(path)
    print(data)
    print(durable)
    return nil
}

fn main() -> i64 {
    let storage: FileStorage = FileStorage { root: "/tmp" }
    let data: str, err: error = Storage.load(storage, "settings.flux")
    if err != nil:
        print(err)
    print(data)
    let saveErr: error = Storage.save(storage, "settings.flux", data, durable: true)
    if saveErr != nil:
        print(saveErr)
    return 0
}

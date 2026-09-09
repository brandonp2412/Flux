interface Storage {
    fn load(path: str) -> (str, error)
    fn label() -> str
}

struct FileStorage {
    root: str
}

struct MemoryStorage {
    name: str
}

impl Storage for FileStorage {
    load: fileLoad
    label: fileLabel
}

impl Storage for MemoryStorage {
    load: memoryLoad
    label: memoryLabel
}

fn fileLoad(storage: FileStorage, path: str) -> (str, error) {
    print(storage.root)
    return path, nil
}

fn fileLabel(storage: FileStorage) -> str {
    return storage.root
}

fn memoryLoad(storage: MemoryStorage, path: str) -> (str, error) {
    print(storage.name)
    return path, nil
}

fn memoryLabel(storage: MemoryStorage) -> str {
    return storage.name
}

fn loadAny(storage: Storage, path: str) -> (str, error) {
    return Storage.load(storage, path)
}

fn labelAny(storage: Storage) -> str {
    return Storage.label(storage)
}

fn selectStorage(memory: bool) -> Storage {
    if memory:
        let ram: MemoryStorage = MemoryStorage { name: "ram" }
        return Storage(ram)
    let file: FileStorage = FileStorage { root: "/tmp" }
    return Storage(file)
}

fn main() -> i64 {
    let storage: Storage = selectStorage(true)
    print(labelAny(storage))
    let data: str, err: error = loadAny(storage, "settings.flux")
    if err != nil:
        print(err)
    print(data)
    return 0
}

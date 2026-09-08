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
    load: file_load
    label: file_label
}

impl Storage for MemoryStorage {
    load: memory_load
    label: memory_label
}

fn file_load(storage: FileStorage, path: str) -> (str, error) {
    print(storage.root)
    return path, nil
}

fn file_label(storage: FileStorage) -> str {
    return storage.root
}

fn memory_load(storage: MemoryStorage, path: str) -> (str, error) {
    print(storage.name)
    return path, nil
}

fn memory_label(storage: MemoryStorage) -> str {
    return storage.name
}

fn load_any(storage: Storage, path: str) -> (str, error) {
    return Storage.load(storage, path)
}

fn label_any(storage: Storage) -> str {
    return Storage.label(storage)
}

fn select_storage(memory: bool) -> Storage {
    let file: FileStorage = FileStorage { root: "/tmp" }
    let ram: MemoryStorage = MemoryStorage { name: "ram" }
    return Storage(ram) if memory else Storage(file)
}

fn main() -> i64 {
    let storage: Storage = select_storage(true)
    print(label_any(storage))
    let data: str, err: error = load_any(storage, "settings.flux")
    if err != nil:
        print(err)
    print(data)
    return 0
}

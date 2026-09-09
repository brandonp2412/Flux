interface Readable {
    fn load(path: str) -> (str, error)
}

interface Writable {
    fn save(path: str, data: str, *, durable: bool) -> error
}

interface Storage: Readable, Writable {
    fn label() -> str
}

struct MemoryStorage {
    label: str
}

fn memoryLoad(storage: MemoryStorage, path: str) -> (str, error) {
    print(storage.label)
    return path, nil
}

fn memorySave(_storage: MemoryStorage, path: str, data: str, *, durable: bool) -> error {
    print(path)
    print(data)
    print(durable)
    return nil
}

fn memoryLabel(storage: MemoryStorage) -> str {
    return storage.label
}

impl Storage for MemoryStorage {
    load: memoryLoad
    save: memorySave
    label: memoryLabel
}

fn main() -> i64 {
    let concrete: MemoryStorage = MemoryStorage { label: "memory" }
    let storage: Storage = Storage(concrete)
    let data: str, err: error = Storage.load(storage, "settings.flux")
    print(data)
    print(err)
    print(Storage.label(storage))
    return 0
}

#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef int64_t (*ApplyFn)(int64_t amount, int64_t value);

typedef struct {
    int64_t amount;
    ApplyFn apply;
} Operation;

static inline int64_t checkedAdd(int64_t left, int64_t right) {
    int64_t result;
    if (__builtin_add_overflow(left, right, &result)) {
        abort();
    }
    return result;
}

static int64_t applyOffset(int64_t amount, int64_t value) {
    return checkedAdd(amount, value);
}

static int64_t run(Operation operation) {
    int64_t total = 0;
    for (int64_t value = 0; value < 100000000; ++value) {
        total = checkedAdd(total, operation.apply(operation.amount, value));
    }
    return total;
}

int main(void) {
    Operation operation = {.amount = 1, .apply = applyOffset};
    printf("%" PRId64 "\n", run(operation));
    return 0;
}

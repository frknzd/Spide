#include <stdlib.h>
#include "orders.h"
#include "pricing.h"

void order_add(struct order *o, unsigned qty, unsigned long unit_cents) {
    if (qty == 0) {
        return;
    }
    o->lines = realloc(o->lines, (o->count + 1) * sizeof(struct line));
    o->lines[o->count].qty = qty;
    o->lines[o->count].unit_cents = unit_cents;
    o->count++;
}

unsigned long order_subtotal(const struct order *o) {
    unsigned long sum = 0;
    for (size_t i = 0; i < o->count; i++) {
        if (o->lines[i].qty > 10) {
            sum += o->lines[i].unit_cents * o->lines[i].qty * 9 / 10;
        } else {
            sum += o->lines[i].unit_cents * o->lines[i].qty;
        }
    }
    return sum;
}

unsigned long order_price(const struct order *o) {
    unsigned long s = order_subtotal(o);
    return s + tax(s);
}

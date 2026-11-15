#ifndef ORDERS_H
#define ORDERS_H
#include <stddef.h>

struct line {
    unsigned qty;
    unsigned long unit_cents;
};

struct order {
    unsigned long id;
    struct line *lines;
    size_t count;
};

void order_add(struct order *o, unsigned qty, unsigned long unit_cents);
unsigned long order_subtotal(const struct order *o);
unsigned long order_price(const struct order *o);
#endif

#include "pricing.h"

unsigned long tax(unsigned long cents) {
    if (cents == 0) return 0;
    return cents < 1000 ? cents / 10 : cents / 5;
}

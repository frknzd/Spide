#include "orders.hpp"

namespace shop {

void Order::add(unsigned qty, std::uint64_t unit_cents) {
    if (qty == 0) {
        return;
    }
    lines_.push_back(Line{qty, unit_cents});
}

std::uint64_t Order::subtotal() const {
    std::uint64_t sum = 0;
    for (const auto& l : lines_) {
        sum += l.qty > 10 ? l.unit_cents * l.qty * 9 / 10 : l.unit_cents * l.qty;
    }
    return sum;
}

std::uint64_t Order::price() const {
    const auto s = subtotal();
    return s + tax(s);
}

std::uint64_t tax(std::uint64_t cents) {
    if (cents == 0) return 0;
    return cents < 1000 ? cents / 10 : cents / 5;
}

}  // namespace shop

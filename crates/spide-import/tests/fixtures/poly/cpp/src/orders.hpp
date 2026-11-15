#pragma once
#include <vector>
#include "pricing.hpp"

namespace shop {

struct Line {
    unsigned qty;
    std::uint64_t unit_cents;
};

class Order : public Priced {
public:
    explicit Order(std::uint64_t id) : id_(id) {}
    void add(unsigned qty, std::uint64_t unit_cents);
    std::uint64_t subtotal() const;
    std::uint64_t price() const override;

private:
    std::uint64_t id_;
    std::vector<Line> lines_;
};

}  // namespace shop

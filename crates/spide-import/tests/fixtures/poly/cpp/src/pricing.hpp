#pragma once
#include <cstdint>

namespace shop {

class Priced {
public:
    virtual ~Priced() = default;
    virtual std::uint64_t price() const = 0;
};

std::uint64_t tax(std::uint64_t cents);

}  // namespace shop

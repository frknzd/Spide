package org.game.domain;

import java.util.ArrayList;
import java.util.List;

public class Board {
    private final List<Square> squares = new ArrayList<>();

    public Board() {
        squares.add(new Square("Go", SquareKind.GO, 0));
        for (int i = 1; i < 40; i++) {
            squares.add(new Square("Square " + i, SquareKind.LOT, 60 + i * 4));
        }
    }

    public Square getSquare(Square from, int steps) {
        int i = squares.indexOf(from);
        return squares.get((i + steps) % squares.size());
    }

    public Square start() { return squares.get(0); }

    public int totalPrice() {
        int total = 0;
        for (Square s : squares) {
            if (s.getPrice() > 0) {
                total += s.getPrice();
            }
        }
        return total;
    }
}

package org.game.domain;

import java.util.ArrayList;
import java.util.List;

public class Game {
    private final Board board = new Board();
    private final List<Player> players = new ArrayList<>();
    private final Cup cup = new Cup();

    public void addPlayer(String name) {
        players.add(new Player(name, new Piece(board.start())));
    }

    public void playRound() {
        for (Player p : players) {
            int roll = cup.roll();
            Square to = board.getSquare(p.getPiece().getLocation(), roll);
            p.getPiece().moveTo(to);
            p.landOn(to);
        }
    }

    /** Rent per kind: the same switch again, in another class. */
    public int rentFor(Square s) {
        switch (s.getKind()) {
            case GO:
                return 0;
            case TAX:
                return 200;
            case JAIL:
                return 0;
            case LOT:
                return s.getPrice() / 10;
        }
        return 0;
    }

    public Square extraSquare() {
        return new Square("Free parking", SquareKind.LOT, 0);
    }

    public int netWorth() {
        int total = 0;
        for (Player p : players) {
            if (p.getCash() > 0) {
                total += p.getCash();
            }
        }
        return total;
    }
}

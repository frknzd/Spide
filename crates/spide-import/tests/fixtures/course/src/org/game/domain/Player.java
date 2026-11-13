package org.game.domain;

import javax.swing.JOptionPane;

public class Player {
    private final String name;
    private final Piece piece;
    private int cash = 1500;

    public Player(String name, Piece piece) {
        this.name = name;
        this.piece = piece;
    }

    public Piece getPiece() { return piece; }
    public String getName() { return name; }
    public int getCash() { return cash; }

    /** Landing behaviour: a switch on the square kind (the GATK smell). */
    public void landOn(Square s) {
        switch (s.getKind()) {
            case GO:
                cash += 200;
                break;
            case TAX:
                cash -= Math.min(200, cash / 10);
                break;
            case JAIL:
                break;
            case LOT:
                if (cash >= s.getPrice()) {
                    cash -= s.getPrice();
                }
                break;
        }
        JOptionPane.showMessageDialog(null, name + " landed on " + s.getName());
    }
}

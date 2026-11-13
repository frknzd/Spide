package org.game.domain;

/** A square on the board; behaviour is chosen by switching on its kind (deliberately not polymorphic). */
public class Square {
    private final String name;
    private final SquareKind kind;
    private final int price;

    public Square(String name, SquareKind kind, int price) {
        this.name = name;
        this.kind = kind;
        this.price = price;
    }

    public String getName() { return name; }
    public SquareKind getKind() { return kind; }
    public int getPrice() { return price; }
}

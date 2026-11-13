package org.game.domain;

public class Piece {
    private Square location;

    public Piece(Square start) { this.location = start; }

    public Square getLocation() { return location; }
    public void moveTo(Square s) { this.location = s; }
}

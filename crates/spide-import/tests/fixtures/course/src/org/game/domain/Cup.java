package org.game.domain;

import java.util.Random;

public class Cup {
    private final Random random = new Random();

    public int roll() { return random.nextInt(6) + 1 + random.nextInt(6) + 1; }
}

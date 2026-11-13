package org.game.ui;

import org.game.domain.Game;
import org.game.domain.Player;
import java.util.List;

/** A UI class that computes standings itself (domain work in the UI). */
public class GamePanel {
    private final Game game;
    private String status = "";

    public GamePanel(Game game) { this.game = game; }

    public void onNextRound() {
        game.playRound();
        refresh();
    }

    public void refresh() {
        status = "round played";
    }

    public int richestCash(List<Player> players) {
        int best = 0;
        for (Player p : players) {
            if (p.getCash() > best) {
                best = p.getCash();
            }
        }
        return best;
    }

    public String locationOf(Player p) {
        return p.getPiece().getLocation().getName();
    }
}

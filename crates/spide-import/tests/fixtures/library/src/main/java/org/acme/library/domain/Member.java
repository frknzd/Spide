package org.acme.library.domain;

import java.util.ArrayList;
import java.util.List;

public class Member {
    private final String name;
    private final List<Loan> loans = new ArrayList<>();

    public Member(String name) { this.name = name; }

    public boolean canBorrow() { return loans.size() < 5; }

    public void add(Loan loan) { loans.add(loan); }

    public String getName() { return name; }
}

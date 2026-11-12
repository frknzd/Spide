package org.acme.library.app;

import org.acme.library.domain.Book;
import org.acme.library.domain.Loan;
import org.acme.library.domain.Member;
import org.acme.library.infra.LoanRepository;

public class LendingService {
    private final LoanRepository loans;

    public LendingService(LoanRepository loans) { this.loans = loans; }

    public Loan lend(Member member, Book book) {
        if (!member.canBorrow()) throw new IllegalStateException();
        Loan loan = new Loan();
        member.add(loan);
        loans.save(loan);
        return loan;
    }
}

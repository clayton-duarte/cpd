//! The shape of the poll's search query (contract §7.6 step 1, A7 pro-d392): the review-requested branch is slim.

use super::gh::SEARCH_QUERY;

/// The two branches of the query: the author search and the review-requested one.
fn branches() -> (&'static str, &'static str) {
    SEARCH_QUERY
        .split_once("reviewRequests: search(")
        .expect("the query has a reviewRequests branch")
}

#[test]
fn review_requested_search_branch_is_slim() {
    let (author, review) = branches();
    for field in ["commits", "latestOpinionatedReviews", "reviewThreads"] {
        assert!(
            author.contains(field),
            "the author branch keeps {field}: {author}"
        );
        assert!(
            !review.contains(field),
            "the review-requested branch carries no {field}: {review}"
        );
    }
}

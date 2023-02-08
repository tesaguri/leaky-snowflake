def_timelines! {
    "/1.1/lists/statuses.json";
    #[derive(Debug, oauth::Request)]
    pub struct Statuses {
        list_id: u64,
        @since_id since_id: Option<u64>,
        count: usize = 200,
        include_entities: bool = false,
    }
}

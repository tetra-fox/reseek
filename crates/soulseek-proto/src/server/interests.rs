//! Interests, recommendations and similar users (spec 01 sections 5.22 to 5.24, 5.36, 01a sections
//! 2.6, 2.7).

use crate::codec::{Wire, wire_struct};
use crate::error::DecodeError;
use crate::wire::{Reader, Writer};

wire_struct! {
  pub struct Recommendation {
    pub interest: String,
    /// signed score, negative for strongly disliked
    pub count: i32,
  }
}

/// Server codes 54 and 56, spec 01 section 5.23
///
/// The two-block layout of `GetPersonalRecommendations`, `GetGlobalRecommendations` and
/// `GetInterestRecommendations`. The second block is optional on read and always written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct Recommendations {
  pub recommendations: Vec<Recommendation>,
  pub unrecommendations: Vec<Recommendation>,
}

impl Wire for Recommendations {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let recommendations = Wire::read_field(r, "recommendations")?;
    let unrecommendations = if r.has_more() {
      Wire::read_field(r, "unrecommendations")?
    } else {
      Vec::new()
    };
    Ok(Self {
      recommendations,
      unrecommendations,
    })
  }

  fn write(&self, w: &mut Writer) {
    self.recommendations.write(w);
    self.unrecommendations.write(w);
  }
}

wire_struct! {
  /// Server code 50, spec 01a section 2.6, obsolete
  pub struct SimilarRecommendationsRequest {
    pub recommendation: String,
  }
}

wire_struct! {
  /// Server code 50, spec 01a section 2.6, obsolete
  pub struct SimilarRecommendationsResponse {
    pub recommendation: String,
    pub similar: Vec<String>,
  }
}

wire_struct! {
  /// Server code 51, spec 01 section 5.22
  pub struct LikedInterestAdd {
    pub interest: String,
  }
}

wire_struct! {
  /// Server code 52, spec 01 section 5.22
  pub struct LikedInterestRemove {
    pub interest: String,
  }
}

wire_struct! {
  /// Server code 55, spec 01a section 2.7, obsolete
  pub struct MyRecommendationsResponse {
    pub likes: Vec<String>,
  }
}

wire_struct! {
  /// Server code 57, spec 01 section 5.24
  pub struct GetUserInterestsRequest {
    pub username: String,
  }
}

wire_struct! {
  /// Server code 57, spec 01 section 5.24
  ///
  /// The `GetUserInterests` reply
  pub struct UserInterests {
    pub username: String,
    pub likes: Vec<String>,
    pub hates: Vec<String>,
  }
}

wire_struct! {
  /// Server code 110, spec 01 section 5.36
  pub struct GetPersonalSimilarUsersResponse {
    pub users: Vec<SimilarUser>,
  }
}

wire_struct! {
  pub struct SimilarUser {
    pub username: String,
    pub rating: u32,
  }
}

wire_struct! {
  /// Server code 111, spec 01 section 5.36
  pub struct GetInterestRecommendationsRequest {
    pub interest: String,
  }
}

wire_struct! {
  /// Server code 111, spec 01 section 5.36
  pub struct GetInterestRecommendationsResponse {
    pub interest: String,
    pub recommendations: Recommendations,
  }
}

wire_struct! {
  /// Server code 112, spec 01 section 5.36
  pub struct GetInterestSimilarUsersRequest {
    pub interest: String,
  }
}

wire_struct! {
  /// Server code 112, spec 01 section 5.36
  pub struct GetInterestSimilarUsersResponse {
    pub interest: String,
    pub usernames: Vec<String>,
  }
}

wire_struct! {
  /// Server code 117, spec 01 section 5.22
  pub struct HatedInterestAdd {
    pub interest: String,
  }
}

wire_struct! {
  /// Server code 118, spec 01 section 5.22
  pub struct HatedInterestRemove {
    pub interest: String,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::server::{ServerRequest as Q, ServerResponse as R};
  use crate::testing::{decodes, p, vectors};

  vectors! {
    similar_recommendations_request:
      Q::SimilarRecommendations(SimilarRecommendationsRequest { recommendation: "jazz".into() }) => p().s("jazz").frame(50);
    similar_recommendations_response:
      R::SimilarRecommendations(SimilarRecommendationsResponse { recommendation: "jazz".into(), similar: vec![] })
      => p().s("jazz").u32(0).frame(50);
    liked_interest_add_request: Q::LikedInterestAdd(LikedInterestAdd { interest: "jazz".into() }) => p().s("jazz").frame(51);
    liked_interest_remove_request:
      Q::LikedInterestRemove(LikedInterestRemove { interest: "jazz".into() }) => p().s("jazz").frame(52);
    get_personal_recommendations_request: Q::GetPersonalRecommendations => p().frame(54);
    get_personal_recommendations_response:
      R::GetPersonalRecommendations(Recommendations {
        recommendations: vec![Recommendation { interest: "jazz".into(), count: 4 }],
        unrecommendations: vec![Recommendation { interest: "noise".into(), count: -2 }],
      })
      => p().u32(1).s("jazz").i32(4).u32(1).s("noise").i32(-2).frame(54);
    my_recommendations_request: Q::MyRecommendations => p().frame(55);
    my_recommendations_response:
      R::MyRecommendations(MyRecommendationsResponse { likes: vec!["jazz".into()] }) => p().u32(1).s("jazz").frame(55);
    get_global_recommendations_request: Q::GetGlobalRecommendations => p().frame(56);
    get_global_recommendations_response:
      R::GetGlobalRecommendations(Recommendations {
        recommendations: vec![Recommendation { interest: "pop".into(), count: 900 }],
        unrecommendations: vec![],
      })
      => p().u32(1).s("pop").i32(900).u32(0).frame(56);
    get_user_interests_request: Q::GetUserInterests(GetUserInterestsRequest { username: "bob".into() }) => p().s("bob").frame(57);
    get_user_interests_response:
      R::GetUserInterests(UserInterests { username: "bob".into(), likes: vec!["jazz".into()], hates: vec!["noise".into()] })
      => p().s("bob").u32(1).s("jazz").u32(1).s("noise").frame(57);
    get_personal_similar_users_request: Q::GetPersonalSimilarUsers => p().frame(110);
    get_personal_similar_users_response:
      R::GetPersonalSimilarUsers(GetPersonalSimilarUsersResponse { users: vec![SimilarUser { username: "bob".into(), rating: 3 }] })
      => p().u32(1).s("bob").u32(3).frame(110);
    get_interest_recommendations_request:
      Q::GetInterestRecommendations(GetInterestRecommendationsRequest { interest: "jazz".into() }) => p().s("jazz").frame(111);
    get_interest_recommendations_response:
      R::GetInterestRecommendations(GetInterestRecommendationsResponse {
        interest: "jazz".into(),
        recommendations: Recommendations {
          recommendations: vec![Recommendation { interest: "bebop".into(), count: 2 }],
          unrecommendations: vec![Recommendation { interest: "metal".into(), count: -1 }],
        },
      })
      => p().s("jazz").u32(1).s("bebop").i32(2).u32(1).s("metal").i32(-1).frame(111);
    get_interest_similar_users_request:
      Q::GetInterestSimilarUsers(GetInterestSimilarUsersRequest { interest: "jazz".into() }) => p().s("jazz").frame(112);
    get_interest_similar_users_response:
      R::GetInterestSimilarUsers(GetInterestSimilarUsersResponse { interest: "jazz".into(), usernames: vec!["bob".into()] })
      => p().s("jazz").u32(1).s("bob").frame(112);
    hated_interest_add_request: Q::HatedInterestAdd(HatedInterestAdd { interest: "noise".into() }) => p().s("noise").frame(117);
    hated_interest_remove_request:
      Q::HatedInterestRemove(HatedInterestRemove { interest: "noise".into() }) => p().s("noise").frame(118);
  }

  decodes! {
    get_personal_recommendations_response_without_second_block: R, p().u32(1).s("jazz").i32(-3).frame(54)
      => Ok(R::GetPersonalRecommendations(Recommendations {
        recommendations: vec![Recommendation { interest: "jazz".into(), count: -3 }],
        unrecommendations: vec![],
      }));
  }
}

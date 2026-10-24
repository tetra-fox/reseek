//! Interests: the ones we like and hate, what the server recommends from them, and users whose
//! interests are like ours. Recommendation lists come back sorted: recommended interests by
//! score from the highest, the rest from the lowest.

use soulseek_proto::server::{
  GetInterestRecommendationsRequest, GetInterestRecommendationsResponse,
  GetInterestSimilarUsersRequest, GetInterestSimilarUsersResponse, GetPersonalSimilarUsersResponse,
  GetUserInterestsRequest, HatedInterestAdd, HatedInterestRemove, LikedInterestAdd,
  LikedInterestRemove, Recommendation, Recommendations, SimilarUser, UserInterests,
};
use soulseek_proto::{ServerCode, ServerRequest};

use super::Client;
use super::request::{interest_key, non_blank, user_key};
use crate::error::ClientError;
use crate::waiter::WaitKey;

impl Client {
  /// adds `interest` to the ones we like
  pub async fn add_liked_interest(&self, interest: &str) -> Result<(), ClientError> {
    non_blank("interest", interest)?;
    self
      .tell(
        &format!("add liked interest {interest}"),
        ServerRequest::LikedInterestAdd(LikedInterestAdd {
          interest: interest.to_owned(),
        }),
      )
      .await
  }

  /// adds `interest` to the ones we hate
  pub async fn add_hated_interest(&self, interest: &str) -> Result<(), ClientError> {
    non_blank("interest", interest)?;
    self
      .tell(
        &format!("add hated interest {interest}"),
        ServerRequest::HatedInterestAdd(HatedInterestAdd {
          interest: interest.to_owned(),
        }),
      )
      .await
  }

  /// removes `interest` from the ones we like
  pub async fn remove_liked_interest(&self, interest: &str) -> Result<(), ClientError> {
    non_blank("interest", interest)?;
    self
      .tell(
        &format!("remove liked interest {interest}"),
        ServerRequest::LikedInterestRemove(LikedInterestRemove {
          interest: interest.to_owned(),
        }),
      )
      .await
  }

  /// removes `interest` from the ones we hate
  pub async fn remove_hated_interest(&self, interest: &str) -> Result<(), ClientError> {
    non_blank("interest", interest)?;
    self
      .tell(
        &format!("remove hated interest {interest}"),
        ServerRequest::HatedInterestRemove(HatedInterestRemove {
          interest: interest.to_owned(),
        }),
      )
      .await
  }

  /// the interests `username` likes and hates
  pub async fn get_user_interests(&self, username: &str) -> Result<UserInterests, ClientError> {
    non_blank("username", username)?;
    self
      .ask(
        &format!("get interests for user {username}"),
        user_key(ServerCode::GetUserInterests, username),
        ServerRequest::GetUserInterests(GetUserInterestsRequest {
          username: username.to_owned(),
        }),
      )
      .await
  }

  /// the interests we like and hate, as the server has them
  pub async fn get_interests(&self) -> Result<UserInterests, ClientError> {
    let username = self.username().ok_or_else(|| {
      ClientError::InvalidOperation("The username for the current user is not yet known".into())
    })?;
    self.get_user_interests(&username).await
  }

  /// interests recommended across the whole network
  pub async fn get_global_recommendations(&self) -> Result<Recommendations, ClientError> {
    let recommendations = self
      .ask(
        "get global recommendations",
        WaitKey::Server(ServerCode::GetGlobalRecommendations),
        ServerRequest::GetGlobalRecommendations,
      )
      .await?;
    Ok(sorted(recommendations))
  }

  /// interests recommended for us from the ones we like and hate
  pub async fn get_personal_recommendations(&self) -> Result<Recommendations, ClientError> {
    let recommendations = self
      .ask(
        "get personal recommendations",
        WaitKey::Server(ServerCode::GetPersonalRecommendations),
        ServerRequest::GetPersonalRecommendations,
      )
      .await?;
    Ok(sorted(recommendations))
  }

  /// interests recommended to those who like `interest`
  pub async fn get_interest_recommendations(
    &self,
    interest: &str,
  ) -> Result<GetInterestRecommendationsResponse, ClientError> {
    non_blank("interest", interest)?;
    let response: GetInterestRecommendationsResponse = self
      .ask(
        &format!("get recommendations for interest {interest}"),
        interest_key(ServerCode::GetInterestRecommendations, interest),
        ServerRequest::GetInterestRecommendations(GetInterestRecommendationsRequest {
          interest: interest.to_owned(),
        }),
      )
      .await?;
    Ok(GetInterestRecommendationsResponse {
      recommendations: sorted(response.recommendations),
      ..response
    })
  }

  /// users whose interests are like ours, with how alike
  pub async fn get_similar_users(&self) -> Result<Vec<SimilarUser>, ClientError> {
    let response: GetPersonalSimilarUsersResponse = self
      .ask(
        "get similar users",
        WaitKey::Server(ServerCode::GetPersonalSimilarUsers),
        ServerRequest::GetPersonalSimilarUsers,
      )
      .await?;
    Ok(response.users)
  }

  /// users who like `interest`
  pub async fn get_interest_similar_users(
    &self,
    interest: &str,
  ) -> Result<GetInterestSimilarUsersResponse, ClientError> {
    non_blank("interest", interest)?;
    self
      .ask(
        &format!("get similar users for interest {interest}"),
        interest_key(ServerCode::GetInterestSimilarUsers, interest),
        ServerRequest::GetInterestSimilarUsers(GetInterestSimilarUsersRequest {
          interest: interest.to_owned(),
        }),
      )
      .await
  }
}

/// Both blocks of a reply merged and split again by score: above zero is recommended, highest
/// first; zero and below is not, lowest first
fn sorted(recommendations: Recommendations) -> Recommendations {
  let (mut recommended, mut unrecommended): (Vec<Recommendation>, Vec<Recommendation>) =
    recommendations
      .recommendations
      .into_iter()
      .chain(recommendations.unrecommendations)
      .partition(|recommendation| recommendation.count > 0);
  recommended.sort_by_key(|recommendation| std::cmp::Reverse(recommendation.count));
  unrecommended.sort_by_key(|recommendation| recommendation.count);
  Recommendations {
    recommendations: recommended,
    unrecommendations: unrecommended,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn scored(interest: &str, count: i32) -> Recommendation {
    Recommendation {
      interest: interest.into(),
      count,
    }
  }

  #[test]
  fn recommendations_are_merged_and_split_by_score() {
    let reply = Recommendations {
      recommendations: vec![scored("jazz", 2), scored("noise", -1), scored("folk", 5)],
      unrecommendations: vec![scored("polka", -4), scored("blues", 0)],
    };
    let sorted = sorted(reply);
    assert_eq!(
      sorted.recommendations,
      [scored("folk", 5), scored("jazz", 2)]
    );
    assert_eq!(
      sorted.unrecommendations,
      [scored("polka", -4), scored("noise", -1), scored("blues", 0)]
    );
  }
}
